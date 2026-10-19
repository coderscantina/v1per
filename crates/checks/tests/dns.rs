mod support;

use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use hickory_resolver::proto::op::{Message, ResponseCode};
use hickory_resolver::proto::rr::rdata::{A, AAAA, CAA, CNAME, MX, NS, PTR, SOA, SRV, TXT};
use hickory_resolver::proto::rr::{Name, RData, Record, RecordType};
use serde_json::json;
use support::definition;
use tokio::net::UdpSocket;
use v1per_checks::{CheckContext, Status, run};

fn name(text: &str) -> Name {
    Name::from_str(text).unwrap()
}

/// Answers from a fixed zone; unknown names get NXDOMAIN.
fn answers(query: &Name, kind: RecordType) -> Option<Vec<RData>> {
    let zone = name("example.test.");
    if *query != zone && *query != name("1.0.0.127.in-addr.arpa.") {
        return None;
    }
    Some(match kind {
        RecordType::A => vec![
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
            RData::A(A(Ipv4Addr::new(192, 0, 2, 2))),
        ],
        RecordType::AAAA => vec![RData::AAAA(AAAA(
            Ipv6Addr::from_str("2001:db8::1").unwrap(),
        ))],
        RecordType::CNAME => vec![RData::CNAME(CNAME(name("target.example.test.")))],
        RecordType::MX => vec![
            RData::MX(MX::new(10, name("mx1.example.test."))),
            RData::MX(MX::new(20, name("mx2.example.test."))),
        ],
        RecordType::NS => vec![RData::NS(NS(name("ns1.example.test.")))],
        RecordType::TXT => vec![
            RData::TXT(TXT::new(vec!["v=spf1 -all".to_owned()])),
            RData::TXT(TXT::new(vec!["part one".to_owned(), "part two".to_owned()])),
        ],
        RecordType::SRV => vec![RData::SRV(SRV::new(1, 5, 443, name("svc.example.test.")))],
        RecordType::PTR => vec![RData::PTR(PTR(name("localhost.")))],
        RecordType::CAA => vec![RData::CAA(CAA::new_issue(
            false,
            Some(name("letsencrypt.org")),
            Vec::new(),
        ))],
        RecordType::SOA => vec![RData::SOA(SOA::new(
            name("ns1.example.test."),
            name("hostmaster.example.test."),
            2026101801,
            7200,
            3600,
            1209600,
            300,
        ))],
        _ => Vec::new(),
    })
}

async fn serve_dns() -> u16 {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut buffer = [0; 4096];
        loop {
            let (len, peer) = socket.recv_from(&mut buffer).await.unwrap();
            let Ok(request) = Message::from_vec(&buffer[..len]) else {
                continue;
            };
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata.recursion_desired = request.metadata.recursion_desired;
            response.metadata.recursion_available = true;
            for query in &request.queries {
                response.add_query(query.clone());
                match answers(&query.name, query.query_type) {
                    Some(data) => {
                        for rdata in data {
                            response.add_answer(Record::from_rdata(query.name.clone(), 60, rdata));
                        }
                    }
                    None => response.metadata.response_code = ResponseCode::NXDomain,
                }
            }
            let _ = socket.send_to(&response.to_vec().unwrap(), peer).await;
        }
    });
    port
}

fn dns(port: u16, hostname: &str, kind: &str) -> v1per_checks::CheckDefinition {
    definition(json!({
        "type": "dns", "timeout": 5, "hostname": hostname, "dns_resolve_type": kind,
        "dns_resolve_server": "127.0.0.1", "port": port,
    }))
}

#[tokio::test]
async fn record_types_format_like_kuma() {
    let port = serve_dns().await;
    let ctx = CheckContext::new();
    let cases = [
        ("example.test", "A", "Records: 192.0.2.1 | 192.0.2.2"),
        ("example.test", "AAAA", "Records: 2001:db8::1"),
        ("example.test", "CNAME", "target.example.test"),
        (
            "example.test",
            "MX",
            "Hostname: mx1.example.test - Priority: 10 | Hostname: mx2.example.test - Priority: 20",
        ),
        ("example.test", "NS", "Servers: ns1.example.test"),
        (
            "example.test",
            "TXT",
            "Records: v=spf1 -all | part one,part two",
        ),
        (
            "example.test",
            "SRV",
            "Name: svc.example.test | Port: 443 | Priority: 1 | Weight: 5",
        ),
        ("127.0.0.1", "PTR", "Records: localhost"),
        ("example.test", "CAA", "Records: letsencrypt.org"),
        (
            "example.test",
            "SOA",
            "NS-Name: ns1.example.test | Hostmaster: hostmaster.example.test | Serial: 2026101801 | Refresh: 7200 | Retry: 3600 | Expire: 1209600 | MinTTL: 300",
        ),
    ];
    for (hostname, kind, expected) in cases {
        let result = run(&dns(port, hostname, kind), &ctx).await;
        assert_eq!(result.status, Status::Up, "{kind}: {}", result.message);
        assert_eq!(result.message, expected, "{kind}");
    }
}

#[tokio::test]
async fn unknown_name_fails_like_node() {
    let port = serve_dns().await;
    let result = run(&dns(port, "missing.test", "A"), &CheckContext::new()).await;

    assert_eq!(result.status, Status::Down);
    assert_eq!(result.message, "queryA ENOTFOUND missing.test");
}
