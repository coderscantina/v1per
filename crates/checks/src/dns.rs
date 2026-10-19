//! Kuma's dns type: resolve one record type against the configured resolvers and report the
//! records in Kuma's `dns_last_result` format.

use std::net::IpAddr;

use hickory_resolver::TokioResolver;
use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolveHosts, ResolverConfig};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::net::{DnsError, NetError};
use hickory_resolver::proto::op::ResponseCode;
use hickory_resolver::proto::rr::{Name, RData, RecordType};
use tokio::time::Instant;

use crate::definition::{DnsCheck, DnsRecordType};
use crate::result::{CheckResult, Failure};

pub(crate) async fn check(dns: &DnsCheck) -> CheckResult {
    let started = Instant::now();
    let servers = match resolver_servers(&dns.dns_resolve_server).await {
        Ok(servers) => servers,
        Err(message) => return CheckResult::down(Failure::Check, message),
    };
    let config = ResolverConfig::from_name_servers(
        servers
            .into_iter()
            .map(|ip| {
                let connections = [ConnectionConfig::udp(), ConnectionConfig::tcp()]
                    .into_iter()
                    .map(|mut connection| {
                        connection.port = dns.port;
                        connection
                    })
                    .collect();
                NameServerConfig::new(ip, true, connections)
            })
            .collect(),
    );
    let mut builder = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
    // Node's Resolver asks the DNS servers only, never the hosts file.
    builder.options_mut().use_hosts_file = ResolveHosts::Never;
    let resolver = match builder.build() {
        Ok(resolver) => resolver,
        Err(error) => return CheckResult::down(Failure::Check, error.to_string()),
    };

    match lookup(&resolver, dns).await {
        Ok(records) if records.is_empty() => {
            CheckResult::down(Failure::Check, error_message(dns, "ENODATA"))
        }
        Ok(records) => {
            let latency = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            CheckResult::up(format_records(dns.dns_resolve_type, &records)).with_latency(latency)
        }
        Err(error) => CheckResult::down(Failure::Check, error_message(dns, code(&error))),
    }
}

/// Kuma accepts a comma-separated list of IPs and hostnames; hostnames resolve via the system.
async fn resolver_servers(list: &str) -> Result<Vec<IpAddr>, String> {
    let entries: Vec<String> = list
        .split(',')
        .map(|entry| entry.chars().filter(|ch| !ch.is_whitespace()).collect())
        .filter(|entry: &String| !entry.is_empty())
        .collect();
    if entries.is_empty() {
        return Err("No Resolver Servers specified. Please specify at least one resolver server like 1.1.1.1 or a hostname".to_owned());
    }
    let mut servers = Vec::new();
    for entry in entries {
        match entry.parse::<IpAddr>() {
            Ok(ip) => servers.push(ip),
            Err(_) => {
                if let Ok(addresses) = tokio::net::lookup_host((entry.as_str(), 0)).await {
                    servers.extend(addresses.map(|address| address.ip()));
                }
            }
        }
    }
    if servers.is_empty() {
        return Err("None of the configured resolver servers could be resolved to an IP address. Please provide a comma-separated list of valid resolver hostnames or IP addresses.".to_owned());
    }
    Ok(servers)
}

enum Records {
    Names(Vec<String>),
    Txt(Vec<String>),
    Mx(Vec<(String, u16)>),
    Srv(Vec<(String, u16, u16, u16)>),
    Caa(Vec<Option<String>>),
    Soa(Box<[String; 7]>),
}

impl Records {
    fn is_empty(&self) -> bool {
        match self {
            Self::Names(list) | Self::Txt(list) => list.is_empty(),
            Self::Mx(list) => list.is_empty(),
            Self::Srv(list) => list.is_empty(),
            Self::Caa(list) => list.is_empty(),
            Self::Soa(_) => false,
        }
    }
}

async fn lookup(resolver: &TokioResolver, dns: &DnsCheck) -> Result<Records, NetError> {
    let name = if dns.dns_resolve_type == DnsRecordType::Ptr {
        let ip: IpAddr = dns
            .hostname
            .parse()
            .map_err(|_| NetError::Msg(format!("Invalid IP address: {}", dns.hostname)))?;
        Name::from(ip)
    } else {
        // Fully qualified, so no search domains are tried, like Node's resolver.
        let mut name = Name::from_utf8(&dns.hostname)?;
        name.set_fqdn(true);
        name
    };
    let lookup = resolver
        .lookup(name, record_type(dns.dns_resolve_type))
        .await?;
    let mut data = lookup.answers().iter().map(|record| &record.data);
    Ok(match dns.dns_resolve_type {
        DnsRecordType::A
        | DnsRecordType::Aaaa
        | DnsRecordType::Cname
        | DnsRecordType::Ns
        | DnsRecordType::Ptr => Records::Names(
            data.filter_map(|rdata| match rdata {
                RData::A(a) if dns.dns_resolve_type == DnsRecordType::A => Some(a.to_string()),
                RData::AAAA(aaaa) if dns.dns_resolve_type == DnsRecordType::Aaaa => {
                    Some(aaaa.to_string())
                }
                RData::CNAME(cname) if dns.dns_resolve_type == DnsRecordType::Cname => {
                    Some(plain(&cname.0))
                }
                RData::NS(ns) if dns.dns_resolve_type == DnsRecordType::Ns => Some(plain(&ns.0)),
                RData::PTR(ptr) if dns.dns_resolve_type == DnsRecordType::Ptr => {
                    Some(plain(&ptr.0))
                }
                _ => None,
            })
            .collect(),
        ),
        // Node returns each TXT record as an array of chunks; Kuma's join prints them comma-joined.
        DnsRecordType::Txt => Records::Txt(
            data.filter_map(|rdata| match rdata {
                RData::TXT(txt) => Some(
                    txt.txt_data
                        .iter()
                        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
                        .collect::<Vec<_>>()
                        .join(","),
                ),
                _ => None,
            })
            .collect(),
        ),
        DnsRecordType::Mx => Records::Mx(
            data.filter_map(|rdata| match rdata {
                RData::MX(mx) => Some((plain(&mx.exchange), mx.preference)),
                _ => None,
            })
            .collect(),
        ),
        DnsRecordType::Srv => Records::Srv(
            data.filter_map(|rdata| match rdata {
                RData::SRV(srv) => Some((plain(&srv.target), srv.port, srv.priority, srv.weight)),
                _ => None,
            })
            .collect(),
        ),
        DnsRecordType::Caa => Records::Caa(
            data.filter_map(|rdata| match rdata {
                RData::CAA(caa) => Some(
                    caa.tag
                        .eq_ignore_ascii_case("issue")
                        .then(|| String::from_utf8_lossy(&caa.value).into_owned()),
                ),
                _ => None,
            })
            .collect(),
        ),
        DnsRecordType::Soa => {
            let soa = data
                .find_map(|rdata| match rdata {
                    RData::SOA(soa) => Some(soa),
                    _ => None,
                })
                .ok_or_else(|| NetError::Msg(error_message(dns, "ENODATA")))?;
            Records::Soa(Box::new([
                plain(&soa.mname),
                plain(&soa.rname),
                soa.serial.to_string(),
                soa.refresh.to_string(),
                soa.retry.to_string(),
                soa.expire.to_string(),
                soa.minimum.to_string(),
            ]))
        }
    })
}

fn record_type(kind: DnsRecordType) -> RecordType {
    match kind {
        DnsRecordType::A => RecordType::A,
        DnsRecordType::Aaaa => RecordType::AAAA,
        DnsRecordType::Cname => RecordType::CNAME,
        DnsRecordType::Mx => RecordType::MX,
        DnsRecordType::Ns => RecordType::NS,
        DnsRecordType::Txt => RecordType::TXT,
        DnsRecordType::Srv => RecordType::SRV,
        DnsRecordType::Ptr => RecordType::PTR,
        DnsRecordType::Caa => RecordType::CAA,
        DnsRecordType::Soa => RecordType::SOA,
    }
}

/// Node reports names without the trailing dot.
fn plain(name: &Name) -> String {
    name.to_ascii().trim_end_matches('.').to_owned()
}

fn format_records(kind: DnsRecordType, records: &Records) -> String {
    match records {
        Records::Names(names) if kind == DnsRecordType::Cname => {
            names.first().cloned().unwrap_or_default()
        }
        Records::Names(names) if kind == DnsRecordType::Ns => {
            format!("Servers: {}", names.join(" | "))
        }
        Records::Names(list) | Records::Txt(list) => format!("Records: {}", list.join(" | ")),
        Records::Mx(list) => list
            .iter()
            .map(|(exchange, priority)| format!("Hostname: {exchange} - Priority: {priority}"))
            .collect::<Vec<_>>()
            .join(" | "),
        Records::Srv(list) => list
            .iter()
            .map(|(name, port, priority, weight)| {
                format!("Name: {name} | Port: {port} | Priority: {priority} | Weight: {weight}")
            })
            .collect::<Vec<_>>()
            .join(" | "),
        Records::Caa(list) => format!(
            "Records: {}",
            list.iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ")
        ),
        Records::Soa(soa) => {
            let [nsname, hostmaster, serial, refresh, retry, expire, minttl] = soa.as_ref();
            format!(
                "NS-Name: {nsname} | Hostmaster: {hostmaster} | Serial: {serial} | Refresh: {refresh} | Retry: {retry} | Expire: {expire} | MinTTL: {minttl}"
            )
        }
    }
}

/// Node's c-ares error code for a failed lookup.
fn code(error: &NetError) -> &'static str {
    match error {
        NetError::Dns(DnsError::NoRecordsFound(records))
            if records.response_code == ResponseCode::NXDomain =>
        {
            "ENOTFOUND"
        }
        NetError::Dns(DnsError::NoRecordsFound(_)) => "ENODATA",
        NetError::Dns(DnsError::ResponseCode(ResponseCode::Refused)) => "EREFUSED",
        NetError::Dns(DnsError::ResponseCode(ResponseCode::ServFail)) => "ESERVFAIL",
        NetError::Timeout => "ETIMEOUT",
        NetError::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
        NetError::Msg(_) => "EINVAL",
        _ => "EBADRESP",
    }
}

/// `queryA ENOTFOUND example.com`, or `getHostByAddr ...` for PTR, as Node words it.
pub(crate) fn error_message(dns: &DnsCheck, code: &str) -> String {
    let method = match dns.dns_resolve_type {
        DnsRecordType::A => "queryA",
        DnsRecordType::Aaaa => "queryAaaa",
        DnsRecordType::Cname => "queryCname",
        DnsRecordType::Mx => "queryMx",
        DnsRecordType::Ns => "queryNs",
        DnsRecordType::Txt => "queryTxt",
        DnsRecordType::Srv => "querySrv",
        DnsRecordType::Ptr => "getHostByAddr",
        DnsRecordType::Caa => "queryCaa",
        DnsRecordType::Soa => "querySoa",
    };
    format!("{method} {code} {}", dns.hostname)
}
