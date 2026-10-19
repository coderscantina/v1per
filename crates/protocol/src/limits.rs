//! Request limits shared by the server and the browser. Exported to `generated/limits.ts`.

macro_rules! limits {
    (
        numbers { $( $(#[doc = $ndoc:literal])* $nname:ident: $nty:ty = $nvalue:expr; )* }
        strings { $( $(#[doc = $sdoc:literal])* $sname:ident = $svalue:literal; )* }
    ) => {
        $( $(#[doc = $ndoc])* pub const $nname: $nty = $nvalue; )*
        $( $(#[doc = $sdoc])* pub const $sname: &str = $svalue; )*

        /// Every numeric limit with its doc text, for export.
        pub const NUMBERS: &[(&str, &str, i64)] = &[
            $( (stringify!($nname), concat!($($ndoc),*), $nname as i64), )*
        ];

        /// Every string constant with its doc text, for export.
        pub const STRINGS: &[(&str, &str, &str)] = &[
            $( (stringify!($sname), concat!($($sdoc),*), $sname), )*
        ];
    };
}

limits! {
    numbers {
        /// Longest accepted idempotency key, in characters.
        IDEMPOTENCY_KEY_MAX_CHARS: usize = 128;
        /// How long a stored command answer can be replayed, in milliseconds.
        IDEMPOTENCY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
        /// Largest command body, in bytes.
        COMMAND_BODY_MAX_BYTES: usize = 64 * 1024;
        /// Longest query string, in bytes.
        QUERY_STRING_MAX_BYTES: usize = 8 * 1024;
        /// Server-side budget for one API request, in milliseconds.
        REQUEST_TIMEOUT_MS: u64 = 30_000;
        /// Login attempts per client address per minute.
        LOGIN_ATTEMPTS_PER_MINUTE: u32 = 10;
        /// Shortest password, in characters.
        PASSWORD_MIN_CHARS: usize = 12;
        /// Longest password, in characters. Bounds the work of one hash.
        PASSWORD_MAX_CHARS: usize = 256;
        /// A login session ends this long after sign-in, in milliseconds.
        SESSION_LIFETIME_MS: i64 = 30 * 24 * 60 * 60 * 1000;
        /// A login session unused for this long ends, in milliseconds.
        SESSION_IDLE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
        /// A password check lets sensitive commands pass for this long, in milliseconds.
        REAUTHENTICATION_WINDOW_MS: i64 = 10 * 60 * 1000;
        /// Time between a correct password and the second factor, in milliseconds.
        LOGIN_CHALLENGE_MS: i64 = 5 * 60 * 1000;
        /// Wrong second factors before a login challenge ends.
        LOGIN_CHALLENGE_ATTEMPTS: u32 = 5;
        /// Digits of a time-based one-time code.
        TOTP_DIGITS: u32 = 6;
        /// Seconds one time-based code is valid for.
        TOTP_PERIOD_SECONDS: u32 = 30;
        /// Recovery codes per person.
        RECOVERY_CODES: usize = 10;
        /// Active API keys per person.
        API_KEYS_PER_USER_MAX: usize = 50;
        /// Longest node, account, API key or instance name, in characters.
        NAME_MAX_CHARS: usize = 100;
        /// Longest email address, in characters.
        EMAIL_MAX_CHARS: usize = 254;
        /// Levels below the root a node may sit at.
        NODE_DEPTH_MAX: u32 = 8;
        /// Days an invitation lasts unless the inviter picks another lifetime.
        INVITATION_LIFETIME_DEFAULT_DAYS: u32 = 7;
        /// Longest invitation lifetime, in days.
        INVITATION_LIFETIME_MAX_DAYS: u32 = 30;
        /// Shortest check and retry interval, in seconds. The reference workload (decision
        /// 0003) is sized for intervals down to 20 seconds.
        MONITOR_INTERVAL_MIN_SECONDS: u32 = 20;
        /// Longest check and retry interval, in seconds (24 days).
        MONITOR_INTERVAL_MAX_SECONDS: u32 = 2_073_600;
        /// Most retries before a failing monitor is down, and most down beats between resent
        /// alerts.
        MONITOR_RETRIES_MAX: u32 = 10_000;
        /// Longest monitor description, in characters.
        MONITOR_DESCRIPTION_MAX_CHARS: usize = 2_000;
        /// Tags per monitor.
        MONITOR_TAGS_MAX: usize = 20;
        /// Longest tag, in characters.
        MONITOR_TAG_MAX_CHARS: usize = 50;
        /// Headers per http check.
        MONITOR_HEADERS_MAX: usize = 50;
        /// Probes per monitor.
        MONITOR_PROBES_MAX: usize = 20;
        /// Shortest push token a person may set, in characters. Imported tokens keep their
        /// length.
        PUSH_TOKEN_MIN_CHARS: usize = 10;
        /// Longest push token, in characters.
        PUSH_TOKEN_MAX_CHARS: usize = 255;
        /// Heartbeats per page when the request names no limit.
        HEARTBEATS_PAGE_DEFAULT: u32 = 25;
        /// Most heartbeats per page.
        HEARTBEATS_PAGE_MAX: u32 = 100;
        /// Most points one latency series may hold.
        LATENCY_POINTS_MAX: u32 = 1_500;
        /// Most daily bars one request may hold.
        DAY_BARS_MAX: u32 = 90;
        /// Longest status page slug, in characters: lowercase letters, digits and inner dashes.
        PAGE_SLUG_MAX_CHARS: usize = 64;
        /// Longest status page description, in characters.
        PAGE_DESCRIPTION_MAX_CHARS: usize = 2_000;
        /// Longest status page footer, in characters.
        PAGE_FOOTER_MAX_CHARS: usize = 1_000;
        /// Largest status page logo, in bytes.
        PAGE_LOGO_MAX_BYTES: usize = 512 * 1024;
        /// Component groups per status page.
        PAGE_GROUPS_MAX: usize = 50;
        /// Components per status page.
        PAGE_COMPONENTS_MAX: usize = 200;
        /// Hostnames one status page may claim.
        PAGE_HOSTNAMES_MAX: usize = 10;
        /// Allowed emails and email domains together, per status page.
        PAGE_ALLOWED_EMAILS_MAX: usize = 200;
        /// Active share tokens per status page.
        PAGE_SHARE_TOKENS_MAX: usize = 50;
        /// Longest grace before an auto-incident is published, in minutes.
        PAGE_GRACE_MAX_MINUTES: u32 = 1_440;
        /// How long an emailed status page sign-in link works, in milliseconds.
        PAGE_EMAIL_LINK_MS: i64 = 15 * 60 * 1000;
        /// Wrong page passwords and sign-in link requests per client address per minute.
        PAGE_ACCESS_ATTEMPTS_PER_MINUTE: u32 = 10;
    }
    strings {
        /// Header every API call sends. Commands without it are refused with `origin_rejected`.
        REQUEST_MARKER_HEADER = "x-v1per-request";
        /// Value of the request marker header.
        REQUEST_MARKER_VALUE = "1";
        /// `Authorization` scheme of API keys: `Authorization: Bearer <secret>`.
        API_KEY_SCHEME = "Bearer";
        /// Web app path that redeems an invitation: `<path>?code=<code>`.
        INVITATION_PATH = "/join";
        /// Path prefix of push URLs, as in Uptime Kuma: `<prefix><token>?status=up&msg=OK&ping=`.
        PUSH_PATH = "/api/push/";
        /// Path prefix of status pages on the instance's own host: `<prefix><slug>`.
        PAGE_PATH = "/s/";
    }
}
