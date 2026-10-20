# Kuma notifications

This contract owns three things: the list of Kuma notification providers and how V1Per imports each one, the behaviour Kuma shares across providers (events, message text, template variables), and the exact output of the eight core providers. The email (`smtp`) and Discord senders must produce the same strings and payloads as Kuma for the same configuration and event. [Decision 0005](../decisions/0005-notifications.md) sets the scope, [decision 0007](../decisions/0007-kuma-import.md) the import.

Source: Uptime Kuma 2.5.0, commit `d9a60dfc73140d15111752e4e8910ed4b54bd9a3` (see `sources/manifest.json`). Every `path:line` below is relative to the Kuma source root.

## Contents

- [Storage](#storage)
- [Providers](#providers)
- [Shared behaviour](#shared-behaviour)
- [Template engine and variables](#template-engine-and-variables)
- [Email (smtp)](#email-smtp)
- [Discord](#discord)
- [Other core providers](#other-core-providers)
- [Import mapping](#import-mapping)
- [V1Per obligations](#v1per-obligations)
- [Not verified](#not-verified)

## Storage

| Table | Columns | Source |
| --- | --- | --- |
| `notification` | `id`, `name` (255), `active` (bool, default true), `user_id`, `is_default` (bool, default false), `config` (TEXT, JSON) | `db/knex_init_db.js:296-303`, `db/old_migrations/patch-notification-config.sql` |
| `monitor_notification` | `id`, `monitor_id`, `notification_id`, both foreign keys with `ON DELETE CASCADE` | `db/knex_init_db.js:306-325` |
| `notification_sent_history` | `id`, `type` (50), `monitor_id`, `days`, unique on all three | `db/knex_init_db.js:373-380` |

How a channel is saved (`server/notification.js:253-281`):

- The whole form object is stored as `config` with `JSON.stringify` (`:272`). It holds `name`, `type`, `isDefault`, `applyExisting` and every provider field. `type` exists only inside `config`, there is no column.
- `name` is copied to the `name` column (`:270`) and `isDefault` to `is_default` (`:273`). Both values also stay inside `config`.
- `applyExisting` is set to `false` before saving (`:266-268`) and, when it was true, links the channel to every monitor of the user (`:314-330`). Older versions stored `true`; the dialog resets it on load (`src/components/NotificationDialog.vue:429-430`).
- `active` is never written by the save path and never read on any send path. `server/client.js:27` only converts it for the UI list. Kuma sends to every linked channel regardless of `active`.
- Channels belong to one user (`user_id`); save, edit and delete filter by it (`server/notification.js:257`, `:290`).
- `is_default` only matters in the monitor editor, which preselects default channels for new monitors (`src/pages/EditMonitor.vue:4031`).

How a channel is loaded for sending: the send paths read `notification.*` joined through `monitor_notification` (`server/model/monitor.js:1526-1531`, `server/util-server.js:929-932`) and pass `JSON.parse(notification.config)` to the provider (`server/model/monitor.js:1508`, `:1563`, `server/model/domain_expiry.js:158`). The test button passes the unsaved form object directly (`server/server.js:1607`). Providers therefore read only `config` keys, never columns.

Dispatch is `providerList[config.type].send(...)`; an unknown type throws `Notification type is not supported` (`server/notification.js:238-244`). Migration `db/knex_migrations/2025-12-29-0000-remove-line-notify.js:1-26` deletes `linenotify` and `line-notify` channels, so a 2.5.0 database holds none.

## Providers

`Notification.init` registers 98 providers (`server/notification.js:117-216`); names must be unique (`:222-224`). The frontend lists the same 98 keys (`src/components/notifications/index.js:104-202`). The count matches decision 0005. Type strings are case sensitive and mixed in style (`smtp`, `SMSEagle`, `rocket.chat`).

Status for V1Per's first release: "core" providers send. "Imported inactive" providers keep their sealed `config` and are marked inactive with a reason (decision 0005).

| `config.type` | Provider file (`name =` line) | V1Per first release |
| --- | --- | --- |
| `alerta` | `server/notification-providers/alerta.js:6` | imported inactive |
| `AlertNow` | `server/notification-providers/alertnow.js:7` | imported inactive |
| `AliyunSMS` | `server/notification-providers/aliyun-sms.js:8` | imported inactive |
| `apprise` | `server/notification-providers/apprise.js:5` | imported inactive |
| `bale` | `server/notification-providers/bale.js:5` | imported inactive |
| `Bark` | `server/notification-providers/bark.js:19` | imported inactive |
| `Bitrix24` | `server/notification-providers/bitrix24.js:6` | imported inactive |
| `Brevo` | `server/notification-providers/brevo.js:5` | imported inactive |
| `CallMeBot` | `server/notification-providers/call-me-bot.js:5` | imported inactive |
| `Cellsynt` | `server/notification-providers/cellsynt.js:5` | imported inactive |
| `clicksendsms` | `server/notification-providers/clicksendsms.js:5` | imported inactive |
| `DingDing` | `server/notification-providers/dingding.js:7` | imported inactive |
| `discord` | `server/notification-providers/discord.js:7` | core |
| `egosms` | `server/notification-providers/egosms.js:5` | imported inactive |
| `Elks` | `server/notification-providers/46elks.js:5` | imported inactive |
| `evolution` | `server/notification-providers/evolution.js:5` | imported inactive |
| `Feishu` | `server/notification-providers/feishu.js:6` | imported inactive |
| `FlashDuty` | `server/notification-providers/flashduty.js:8` | imported inactive |
| `Flowtriq` | `server/notification-providers/flowtriq.js:6` | imported inactive |
| `fluxer` | `server/notification-providers/fluxer.js:6` | imported inactive |
| `FreeMobile` | `server/notification-providers/freemobile.js:5` | imported inactive |
| `GoAlert` | `server/notification-providers/goalert.js:6` | imported inactive |
| `GoogleChat` | `server/notification-providers/google-chat.js:7` | imported inactive |
| `GoogleSheets` | `server/notification-providers/google-sheets.js:6` | imported inactive |
| `gorush` | `server/notification-providers/gorush.js:5` | imported inactive |
| `gotify` | `server/notification-providers/gotify.js:5` | imported inactive |
| `GrafanaOncall` | `server/notification-providers/grafana-oncall.js:6` | imported inactive |
| `gtxmessaging` | `server/notification-providers/gtx-messaging.js:5` | imported inactive |
| `HaloPSA` | `server/notification-providers/HaloPSA.js:12` | imported inactive |
| `HeiiOnCall` | `server/notification-providers/heii-oncall.js:7` | imported inactive |
| `HomeAssistant` | `server/notification-providers/home-assistant.js:7` | imported inactive |
| `JiraServiceManagement` | `server/notification-providers/jira-service-management.js:8` | imported inactive |
| `Keep` | `server/notification-providers/keep.js:5` | imported inactive |
| `Kook` | `server/notification-providers/kook.js:5` | imported inactive |
| `line` | `server/notification-providers/line.js:6` | imported inactive |
| `lunasea` | `server/notification-providers/lunasea.js:6` | imported inactive |
| `matrix` | `server/notification-providers/matrix.js:7` | imported inactive |
| `mattermost` | `server/notification-providers/mattermost.js:6` | imported inactive |
| `max` | `server/notification-providers/max.js:5` | imported inactive |
| `nextcloudtalk` | `server/notification-providers/nextcloudtalk.js:8` | imported inactive |
| `nostr` | `server/notification-providers/nostr.js:8` | imported inactive |
| `notifery` | `server/notification-providers/notifery.js:7` | imported inactive |
| `ntfy` | `server/notification-providers/ntfy.js:6` | core |
| `octopush` | `server/notification-providers/octopush.js:5` | imported inactive |
| `OneBot` | `server/notification-providers/onebot.js:5` | imported inactive |
| `OneChat` | `server/notification-providers/onechat.js:6` | imported inactive |
| `Onesender` | `server/notification-providers/onesender.js:5` | imported inactive |
| `Ooredoo` | `server/notification-providers/ooredoo.js:9` | imported inactive |
| `Opsgenie` | `server/notification-providers/opsgenie.js:10` | imported inactive |
| `PagerDuty` | `server/notification-providers/pagerduty.js:8` | imported inactive |
| `PagerTree` | `server/notification-providers/pagertree.js:8` | imported inactive |
| `plivo` | `server/notification-providers/plivo.js:5` | imported inactive |
| `promosms` | `server/notification-providers/promosms.js:5` | imported inactive |
| `pumble` | `server/notification-providers/pumble.js:6` | imported inactive |
| `pushbullet` | `server/notification-providers/pushbullet.js:7` | imported inactive |
| `PushByTechulus` | `server/notification-providers/techulus-push.js:5` | imported inactive |
| `PushDeer` | `server/notification-providers/pushdeer.js:6` | imported inactive |
| `pushover` | `server/notification-providers/pushover.js:9` | core |
| `PushPlus` | `server/notification-providers/pushplus.js:6` | imported inactive |
| `pushy` | `server/notification-providers/pushy.js:5` | imported inactive |
| `Resend` | `server/notification-providers/resend.js:5` | imported inactive |
| `rocket.chat` | `server/notification-providers/rocket-chat.js:8` | imported inactive |
| `SendGrid` | `server/notification-providers/send-grid.js:5` | imported inactive |
| `ServerChan` | `server/notification-providers/serverchan.js:6` | imported inactive |
| `serwersms` | `server/notification-providers/serwersms.js:5` | imported inactive |
| `SevenIO` | `server/notification-providers/sevenio.js:6` | imported inactive |
| `signal` | `server/notification-providers/signal.js:5` | imported inactive |
| `SIGNL4` | `server/notification-providers/signl4.js:6` | imported inactive |
| `slack` | `server/notification-providers/slack.js:8` | core |
| `smsc` | `server/notification-providers/smsc.js:5` | imported inactive |
| `SMSEagle` | `server/notification-providers/smseagle.js:5` | imported inactive |
| `smsir` | `server/notification-providers/smsir.js:5` | imported inactive |
| `SMSManager` | `server/notification-providers/smsmanager.js:5` | imported inactive |
| `SMSPartner` | `server/notification-providers/smspartner.js:5` | imported inactive |
| `SMSPlanet` | `server/notification-providers/sms-planet.js:5` | imported inactive |
| `smtp` | `server/notification-providers/smtp.js:6` | core |
| `Splunk` | `server/notification-providers/splunk.js:8` | imported inactive |
| `SpugPush` | `server/notification-providers/spugpush.js:6` | imported inactive |
| `squadcast` | `server/notification-providers/squadcast.js:6` | imported inactive |
| `stackfield` | `server/notification-providers/stackfield.js:7` | imported inactive |
| `teams` | `server/notification-providers/teams.js:7` | core |
| `telegram` | `server/notification-providers/telegram.js:5` | core |
| `telnyx` | `server/notification-providers/telnyx.js:5` | imported inactive |
| `Teltonika` | `server/notification-providers/teltonika.js:10` | imported inactive |
| `threema` | `server/notification-providers/threema.js:5` | imported inactive |
| `twilio` | `server/notification-providers/twilio.js:5` | imported inactive |
| `VK` | `server/notification-providers/vk.js:5` | imported inactive |
| `VKTeams` | `server/notification-providers/vkteams.js:5` | imported inactive |
| `waha` | `server/notification-providers/waha.js:5` | imported inactive |
| `webhook` | `server/notification-providers/webhook.js:6` | core |
| `Webpush` | `server/notification-providers/Webpush.js:6` | imported inactive |
| `WeCom` | `server/notification-providers/wecom.js:6` | imported inactive |
| `whapi` | `server/notification-providers/whapi.js:5` | imported inactive |
| `Whatsapp360messenger` | `server/notification-providers/360messenger.js:5` | imported inactive |
| `WPush` | `server/notification-providers/wpush.js:6` | imported inactive |
| `WxPusher` | `server/notification-providers/wxpusher.js:6` | imported inactive |
| `YZJ` | `server/notification-providers/yzj.js:6` | imported inactive |
| `ZohoCliq` | `server/notification-providers/zoho-cliq.js:6` | imported inactive |

## Shared behaviour

### Provider interface

`send(notification, msg, monitorJSON = null, heartbeatJSON = null)` returns `"Sent Successfully."` or throws (`server/notification-providers/notification-provider.js:23-25`, for example `smtp.js:12`). `notification` is the parsed `config`.

### Events

| Event | `msg` | `monitorJSON` | `heartbeatJSON` | Source |
| --- | --- | --- | --- | --- |
| Monitor down | `[<monitor name>] [🔴 Down] <heartbeat msg>` | `monitor.toJSON(preloadData, false)` | heartbeat plus extras, see below | `server/model/monitor.js:1456-1463`, `:1505-1512` |
| Monitor up | `[<monitor name>] [✅ Up] <heartbeat msg>` | same | same | same |
| Test | `<config name> Testing` | `null` | `null` | `server/server.js:1607` |
| Certificate expiry | `[<monitor name>][<monitor url>] <certType> certificate <subject CN> will expire in <daysRemaining> days` | `null` | `null` | `server/model/monitor.js:1562-1565` |
| Domain expiry | `Domain name <domain> will expire in <daysRemaining> days` | `null` | `null` | `server/model/domain_expiry.js:157-160` |

Details:

- Any status other than UP gets the `🔴 Down` label (`server/model/monitor.js:1457-1461`); in practice only UP and DOWN reach this code.
- `msg` interpolates the raw heartbeat message before the `N/A` substitution (`:1463` runs before `:1469-1471`). An empty heartbeat message leaves a trailing space after `]`.
- `certType` is one of `server`, `intermediate CA`, `root CA`, `self-signed` (`server/util-server.js:477-486`).
- `daysRemaining` is `dayjs.utc(validTo).diff(dayjs.utc(), "day")` for certificates (`server/util-server.js:471`) and `dayjs.utc(expiry).diff(dayjs.utc(), "day")` for domains (`server/model/domain_expiry.js:262-264`), so it truncates toward zero.
- Providers detect test, certificate and domain messages by `heartbeatJSON == null` (for example `discord.js:41-42`, `ntfy.js:34-35`, `teams.js:249`).

### When up and down are sent

- A beat is notified when `isImportantForNotification` holds: first beat, UP to DOWN, DOWN to UP, PENDING to DOWN, MAINTENANCE to DOWN (`server/model/monitor.js:1420-1443`).
- `sendNotification` then skips the first beat unless it is DOWN (`:1453`). A first beat UP sends nothing.
- Push monitors use the same two functions from the push endpoint (`server/routers/api-router.js:102-117`).
- A send failure is logged and the loop continues with the next channel; there is no retry (`server/model/monitor.js:1513-1516`).

### Resend

`monitor.resendInterval` counts beats, not minutes. `downCount` carries over from the previous heartbeat (`server/model/monitor.js:457`). On each non-important DOWN beat it increments, and when `downCount >= resendInterval` (and `resendInterval > 0`) the full down notification is sent again with the same `msg` format and `downCount` resets to 0 (`:989-1001`). An important beat also resets it (`:979`). The repeat is indistinguishable from the first alert.

### Certificate expiry

- Runs on every TLS info update when the monitor has `expiryNotification` on and `ignoreTls` off (`server/model/monitor.js:2072-2079`).
- Thresholds come from setting `tlsExpiryNotifyDays`, default `[7, 14, 21]`, reset to the default if missing or not an array (`server/util-server.js:940-945`). The list is not sorted.
- For each threshold, Kuma walks the chain from the server certificate up, stops at a known root (`:951-956`), and for each certificate with `daysRemaining <= targetDays` calls `sendCertNotificationByTargetDays` (`:947-977`).
- That function skips when `notification_sent_history` has a row with `type = 'certificate'`, this monitor and `days <= targetDays` (`server/model/monitor.js:1545-1554`). After at least one channel succeeds it inserts `targetDays` (`:1573-1579`).
- The history for a monitor is cleared when the server certificate fingerprint changes (`server/model/monitor.js:1273-1278`).

### Domain expiry

New in the 2.x line and present in 2.5.0. Decision 0005 does not list it as a V1Per event.

- Runs on each non-maintenance beat when the monitor has `domainExpiryNotification` on (`server/model/monitor.js:1005-1013`).
- Thresholds come from setting `domainExpiryNotifyDays`, default `[7, 14, 21]`, sorted ascending (`server/model/domain_expiry.js:329-337`).
- Kuma walks the sorted thresholds and skips those with `daysRemaining > targetDays` and those where `lastExpiryNotificationSent <= targetDays`. The first remaining threshold triggers one send to all channels; on any success it is stored in `lastExpiryNotificationSent` on the `domain_expiry` row and the walk stops (`:338-362`).

### Heartbeat JSON

Built by `bean.toJSONAsync({ decodeResponse: true })` (`server/model/heartbeat.js:51-63`) and extended in `sendNotification`.

| Key | Type | Value | Source |
| --- | --- | --- | --- |
| `monitorID` | number | monitor id | `heartbeat.js:53` |
| `status` | number | `0` DOWN, `1` UP, `2` PENDING, `3` MAINTENANCE | `src/util.ts:32-35` |
| `time` | string | UTC time from `R.isoDateTimeMillis(dayjs.utc())` | `server/model/monitor.js:455` |
| `msg` | string | check message, `"N/A"` when empty | `server/model/monitor.js:1469-1471` |
| `ping` | number or null | response time in ms | `heartbeat.js:57` |
| `important` | boolean | true for state changes | `server/model/monitor.js:966` |
| `duration` | number | seconds since the previous beat | `heartbeat.js:59` |
| `retries` | number | retry counter | `heartbeat.js:60` |
| `response` | string or null | stored response body, brotli and base64 decoded | `heartbeat.js:61`, `:70-81` |
| `timezone` | string | `process.env.TZ`, else setting `serverTimezone`, else guessed, else `UTC` | `server/model/monitor.js:1474`, `server/uptime-kuma-server.js:406-445` |
| `timezoneOffset` | string | `dayjs().format("Z")`, for example `+02:00` | `server/model/monitor.js:1475`, `server/uptime-kuma-server.js:451-453` |
| `localDateTime` | string | `time` converted to `timezone`, format `YYYY-MM-DD HH:mm:ss` | `server/model/monitor.js:1476-1479`, `src/util.ts:43` |
| `lastDownTime` | string | UP only: `time` of the latest heartbeat with `status = 0 AND important = 1`, omitted when none | `server/model/monitor.js:1483-1503` |

### Monitor JSON

`monitor.toJSON(preloadData, false)` (`server/model/monitor.js:116-259`). With `includeSensitiveData = false` it omits credentials, headers, body and TLS keys (`:221-255`) and sets `includeSensitiveData: false` (`:257`). Fields providers read: `id`, `name`, `type`, `subtype`, `url`, `hostname`, `port`, `path` (array of group names ending with the monitor name), `pathName`, `tags` (array of `{name, value, color, ...}`).

### Address helper

`extractAddress(monitorJSON)` (`notification-provider.js:32-65`):

| Monitor type | Result |
| --- | --- |
| `monitorJSON` null | `""` |
| `push` | `"Heartbeat"` |
| `ping`, `dns` | `hostname` |
| `port`, `gamedig`, `steam` | `hostname:port`, or `hostname` when `port` is falsy |
| `globalping` | `hostname` for subtype `ping`/`dns`, `url` for `http`, else `""` |
| anything else | `url` unless it is `https://`, `http://` or `""`, then `""` |

### Error helper

`throwGeneralAxiosError(error)` (`notification-provider.js:121-168`) throws one `Error` whose message is `error.message`, then ` (code=<code>)`, then ` (HTTP <status> <statusText>)`, then the response body (string as is, otherwise JSON), then either ` - caused by: <m1>; <m2>` for an `AggregateError` or ` - cause: <message>`. Kuma shows this text in the test dialog and logs it.

### Proxy

`getAxiosConfigWithProxy` reads env `notification_proxy` or `NOTIFICATION_PROXY` and installs an HTTP, HTTPS or SOCKS agent (`notification-provider.js:175-196`). All core HTTP providers use it, including the Discord avatar check (`discord.js:21`, `:31`).

## Template engine and variables

Engine: `liquidjs`, declared `~10.26.0` (`package.json:114`), locked at `10.26.0` (`package-lock.json:10537-10538`). Each render builds a new engine (`notification-provider.js:75-81`) with:

```js
new Liquid({ root: "./no-such-directory-uptime-kuma", relativeReference: false, dynamicPartials: false })
```

All other options are liquidjs defaults. The `root` points to a directory that does not exist, so `{% include %}` and `{% render %}` cannot read files. The template is parsed with `engine.parse` and rendered with `engine.render` (`:81`, `:112`). A parse or render error throws and fails that send.

Context (`notification-provider.js:84-110`):

| Variable | Type | Value |
| --- | --- | --- |
| `status` | string | `"🔴 Down"` when `heartbeatJSON.status === 0`, `"✅ Up"` for any other heartbeat, `"⚠️ Test"` when `heartbeatJSON` is null |
| `name` | string | `monitorJSON.name`, or `"Monitor Name not available"` when `monitorJSON` is null |
| `hostnameOrURL` | string | `extractAddress(monitorJSON)`, or `"testing.hostname"` when `monitorJSON` is null |
| `monitorJSON` | object or null | as passed |
| `heartbeatJSON` | object or null | as passed |
| `msg` | string | as passed |
| `STATUS`, `NAME`, `HOSTNAME_OR_URL` | string | copies of `status`, `name`, `hostnameOrURL`, marked "for v1 compatibility, to be removed in v3" (`:98-101`) |

Since certificate and domain expiry pass null for both objects, their templates see `"⚠️ Test"`, `"Monitor Name not available"` and `"testing.hostname"`. The editor help claims `monitorJSON` is available for certificate notifications (`src/components/TemplatedTextarea.vue:23-25`); the server does not pass it (`server/model/monitor.js:1562-1565`).

Providers that render templates: `smtp`, `discord`, `webhook`, `slack`, `telegram`, `ntfy` among the core set.

## Email (smtp)

Provider: `server/notification-providers/smtp.js`. Transport: `nodemailer`, declared `~7.0.13` (`package.json:125`), locked at `7.0.13` (`package-lock.json:11688-11689`). A new transport is created per send (`smtp.js:92`).

### Config keys

| Key | Type | Use | Frontend default | Source |
| --- | --- | --- | --- | --- |
| `smtpHost` | string | transport `host` | none, required | `smtp.js:15`, `SMTP.vue:5` |
| `smtpPort` | number | transport `port` | none, required | `smtp.js:16`, `SMTP.vue:26` |
| `smtpSecure` | boolean | transport `secure` (`true` implicit TLS, `false` plain or STARTTLS) | `false` when undefined | `smtp.js:17`, `SMTP.vue:38-41`, `:326-328` |
| `smtpIgnoreTLSError` | boolean | `tls.rejectUnauthorized = !smtpIgnoreTLSError` | unset | `smtp.js:33-35`, `SMTP.vue:48` |
| `smtpIgnoreSTARTTLS` | boolean | when `smtpSecure` is falsy and this is true, set `ignoreTLS: true` and skip `tls` | unset, shown only when `smtpSecure` is false | `smtp.js:22-29`, `SMTP.vue:59-63` |
| `smtpUsername` | string | `auth.user` | unset | `smtp.js:51-56` |
| `smtpPassword` | string | `auth.pass` | unset | `smtp.js:51-56` |
| `smtpFrom` | string | `from` | placeholder only | `smtp.js:94`, `SMTP.vue:102-107` |
| `smtpTo` | string | `to`, comma separated | unset | `smtp.js:97`, `SMTP.vue:116` |
| `smtpCC` | string | `cc` | unset | `smtp.js:95`, `SMTP.vue:129` |
| `smtpBCC` | string | `bcc` | unset | `smtp.js:96`, `SMTP.vue:141` |
| `customSubject` | string | Liquid subject template | unset | `smtp.js:80`, `SMTP.vue:153` |
| `customBody` | string | Liquid body template | unset | `smtp.js:81`, `SMTP.vue:164` |
| `htmlBody` | boolean | send the custom body as `html` | unset | `smtp.js:86`, `SMTP.vue:175` |
| `smtpAdditionalHeaders` | string (JSON object) | extra headers | unset | `smtp.js:59-68`, `SMTP.vue:203` |
| `smtpDkimDomain` | string | `dkim.domainName`; DKIM is on only when this is truthy | unset | `smtp.js:39-48` |
| `smtpDkimKeySelector` | string | `dkim.keySelector` | unset | `smtp.js:42` |
| `smtpDkimPrivateKey` | string | `dkim.privateKey` | unset | `smtp.js:43` |
| `smtpDkimHashAlgo` | string | `dkim.hashAlgo` | unset (placeholder `sha256`) | `smtp.js:44`, `SMTP.vue:254-258` |
| `smtpDkimheaderFieldNames` | string | `dkim.headerFieldNames` | unset | `smtp.js:45` |
| `smtpDkimskipFields` | string | `dkim.skipFields` | unset | `smtp.js:46` |

Key spelling is exact, including the lowercase `h` and `s` in `smtpDkimheaderFieldNames` and `smtpDkimskipFields`. The form requires at least one of `smtpTo`, `smtpCC`, `smtpBCC` (`SMTP.vue:145`, computed `hasRecipient`).

### Transport options

```js
{
  host, port, secure, headers: {},
  // either
  ignoreTLS: true,                                  // smtpSecure falsy and smtpIgnoreSTARTTLS truthy
  // or
  tls: { rejectUnauthorized: !smtpIgnoreTLSError },
  dkim: { domainName, keySelector, privateKey, hashAlgo, headerFieldNames, skipFields }, // only with smtpDkimDomain
  auth: { user, pass },                             // only when username or password is truthy
}
```

`smtp.js:14-56`. The disabled STARTTLS path logs a warning (`:25-28`). Additional headers are parsed with `JSON.parse` and merged into `config.headers`; invalid JSON throws `Additional Headers is not a valid JSON` (`:59-68`).

### Message

`transporter.sendMail({ from, cc, bcc, to, subject, headers: config.headers, [text|html]: body })` (`smtp.js:93-102`).

Subject and body (`smtp.js:70-89`):

1. Defaults: `subject = msg`, `body = msg`, plain text.
2. When `heartbeatJSON` is set (up and down), `body = msg + "\n" + "Time (" + timezone + "): " + localDateTime`, for example `[api.example.com] [🔴 Down] timeout of 48000ms exceeded\nTime (Europe/Vienna): 2026-10-18 14:03:11`.
3. Templates apply only when both `monitorJSON` and `heartbeatJSON` are set, or when `msg` ends with `Testing`. Certificate and domain expiry mails never use templates.
4. `customSubject` and `customBody` are trimmed; an empty result keeps the default. A non-empty subject is rendered with Liquid.
5. A non-empty body is rendered with Liquid, and only then does `htmlBody` take effect. Without a custom body the mail is always `text`, even if `htmlBody` is true.

Default output per event:

| Event | Subject | Body (`text`) |
| --- | --- | --- |
| Down | `[<name>] [🔴 Down] <msg>` | subject line, newline, `Time (<timezone>): <localDateTime>` |
| Up | `[<name>] [✅ Up] <msg>` | same shape |
| Test | `<channel name> Testing` | `<channel name> Testing` |
| Certificate | certificate `msg` | same |
| Domain | domain `msg` | same |

## Discord

Provider: `server/notification-providers/discord.js`. All requests use `axios` (`~0.32.0`, `package.json:77`) with the proxy config.

### Config keys

| Key | Type | Use | Frontend default | Source |
| --- | --- | --- | --- | --- |
| `discordWebhookUrl` | string | webhook URL, parsed with `new URL` | none, required | `discord.js:23`, `Discord.vue:6` |
| `discordUsername` | string | `username`, fallback `"Uptime Kuma"` | placeholder app name | `discord.js:22`, `Discord.vue:20-24` |
| `discordPrefixMessage` | string | `content` on normal down and up embeds | unset | `discord.js:168-170`, `:248-250` |
| `discordMessageFormat` | `"normal"`, `"minimalist"`, `"custom"` | message layout | `"custom"` if the legacy flag or a template exists, else `"normal"` | `discord.js:38-39`, `Discord.vue:42-45`, `:165-170` |
| `discordUseMessageTemplate` | boolean | legacy: when `discordMessageFormat` is unset, true selects `"custom"` | none | `discord.js:39` |
| `discordMessageTemplate` | string | Liquid template for `"custom"`, trimmed | unset | `discord.js:47`, `:96-104` |
| `discordChannelType` | `"channel"`, `"createNewForumPost"`, `"postToThread"` | target | `"channel"` when falsy | `discord.js:24`, `Discord.vue:64-67`, `:155-157` |
| `threadId` | string | `thread_id` query parameter for `postToThread` | unset | `discord.js:24-26`, `Discord.vue:95` |
| `postName` | string | `thread_name` for `createNewForumPost` | unset | `discord.js:59-61`, `Discord.vue:78` |
| `disableUrl` | boolean | hide the URL field in embeds | `false` when undefined | `discord.js:137`, `:198`, `Discord.vue:158-160` |
| `discordSuppressNotifications` | boolean | `flags = 4096` (`1 << 12`, SUPPRESS_NOTIFICATIONS) | `false` when undefined | `discord.js:18`, `:62-64`, `Discord.vue:161-163` |

The server does not apply the frontend defaults. A config saved without `discordMessageFormat` and without `discordUseMessageTemplate` behaves as `"normal"` even if `discordMessageTemplate` is set (`discord.js:38-39`); the dialog would show it as `"custom"` (`Discord.vue:165-170`).

### Request flow

1. Build the URL; for `postToThread` append `thread_id=<threadId>` (`discord.js:23-26`).
2. `GET` that URL. If the response has a falsy `avatar`, add `avatar_url: "https://github.com/louislam/uptime-kuma/raw/master/public/icon.png"` to the payload. If the `GET` fails, assume an avatar exists (`:28-36`).
3. `POST` the JSON payload to the same URL (`:65`, `:91`, `:118`, `:175`, `:255`).
4. Any error goes through `throwGeneralAxiosError` (`:258-260`), including an invalid URL.

Payload keys are added in this order: `username`, `content` or `embeds`, `avatar_url`, `thread_name`, `content` (prefix, normal embeds only), `flags`. `thread_name` is set only for `createNewForumPost`, `flags` only when `discordSuppressNotifications` is truthy.

### Test, certificate and domain messages (`heartbeatJSON` null)

`discord.js:42-67`. Plain `content`, no embeds, no prefix message:

| Format | `content` |
| --- | --- |
| `normal` | `msg` |
| `minimalist` | `"Test: " + msg` (also for certificate and domain messages) |
| `custom` | rendered `discordMessageTemplate`, or `msg` when the trimmed template is empty |

### Minimalist up and down

`discord.js:73-93`. `content` is `"🔴 <name> is down."` for DOWN, otherwise `"🟢 <name> is up."`. No embeds, no prefix.

### Custom up and down

`discord.js:96-120`. When the format is `custom` and the trimmed template is non-empty, `content` is the rendered template. No embeds, no prefix. With an empty template, normal embeds are sent.

### Normal down embed

`discord.js:122-176`.

| Part | Value |
| --- | --- |
| `title` | `"❌ Your service " + name + " went down. ❌"` |
| `color` | `16711680` (`#FF0000`) |
| `timestamp` | `heartbeatJSON.time` as is |
| field | `Service Name`: monitor name |
| field (optional) | `Service Type` for push monitors, else `Service URL`: `extractAddress(monitorJSON)`; omitted when `disableUrl` is truthy or the address is empty |
| field | `Went Offline`: `<t:<unix seconds of heartbeat time>:F>` |
| field | `Time (<timezone>)`: `localDateTime` |
| field | `Error`: `heartbeatJSON.msg`, or `N/A` when null |
| `content` | `discordPrefixMessage` when truthy |

No `footer`, `author`, `url` or `inline` flags are set.

### Normal up embed

`discord.js:177-257`.

| Part | Value |
| --- | --- |
| `title` | `"✅ Your service " + name + " is up! ✅"` |
| `color` | `65280` (`#00FF00`) |
| `timestamp` | `heartbeatJSON.time` as is |
| field | `Service Name`: monitor name |
| field (optional) | `Service Type` or `Service URL`, same rule as down |
| field (optional) | `Went Offline`: `<t:<unix seconds of lastDownTime>:F>`, only when `lastDownTime` is set |
| field (optional) | `Downtime Duration`: `formatDuration(upSeconds - downSeconds)`, only when `lastDownTime` is set |
| field | `Time (<timezone>)`: `localDateTime` |
| field (optional) | `Ping`: `<ping> ms`, only when `ping` is not null |
| `content` | `discordPrefixMessage` when truthy |

Both timestamps come from `dayjs.utc(...).unix()` (`:178-183`). `formatDuration(seconds)` (`:269-287`) joins `<h>h`, `<m>m` and `<s>s` with spaces, omitting zero parts, shows seconds only when hours are 0, and returns `0s` when all parts are zero. Examples: `3725` gives `1h 2m`, `95` gives `1m 35s`.

A heartbeat with a status other than UP or DOWN reaches neither branch and sends nothing (`:122`, `:177`).

## Other core providers

All post JSON with `axios` and the proxy config unless stated.

### webhook

`server/notification-providers/webhook.js`. Keys: `webhookURL`, `httpMethod` (`post` or `get`, server default `post`, `:15`; frontend sets `post`, `Webhook.vue:108-112`), `webhookContentType` (`json`, `form-data`, `custom`; `Webhook.vue:33-35`), `webhookCustomBody`, `webhookAdditionalHeaders` (JSON string).

| Mode | Request | Source |
| --- | --- | --- |
| `get` | `GET webhookURL` with query `msg`, plus `heartbeat` and `monitor` as JSON strings when not null | `:26-37`, `:60-61` |
| `json` or unset | `POST` body `{"heartbeat": heartbeatJSON, "monitor": monitorJSON, "msg": msg}` | `:17-21`, `:63` |
| `form-data` | `POST` multipart with one field `data` holding that object as JSON, headers from `form-data` | `:38-42` |
| `custom` | `POST` the rendered `webhookCustomBody` as a string, untrimmed | `:43-45` |

Additional headers merge over the computed ones; invalid JSON throws `Additional Headers is not a valid JSON` (`:47-56`). Test, certificate and domain messages send `heartbeat: null, monitor: null`.

### slack

`server/notification-providers/slack.js`. Keys: `slackwebhookURL`, `slackchannel`, `slackusername`, `slackiconemo`, `slackchannelnotify`, `slackrichmessage`, `slackUseTemplate`, `slackTemplate`, `slackIncludeGroupName` (server default `true`, `:182`; frontend sets `true`, `Slack.vue:126-130`), legacy `slackbutton`.

- `slackchannelnotify` appends ` <!channel>` to `msg` before anything else (`:143-145`).
- Test, certificate, domain: `{text: msg, channel, username, icon_emoji}` (`:149-158`).
- Template on: same shape with `text` = rendered `slackTemplate` (`:163-180`).
- Plain: `text` = `_<group path>_\n<name>\n<msg>` when the monitor sits in a group and group names are on, else `<name>\n<msg>`; `attachments: []` (`:182-208`). Group path is `monitorJSON.path` without the last element, joined with ` / `.
- Rich (`slackrichmessage`): `text: msg` and one attachment with `color` `#2eb886` (UP) or `#e01e5a`, blocks: `header` (monitor name), optional `context` with `_<group path>_`, `section` with fields `*Message*\n<msg>` and `*Time (<timezone>)*\n<localDateTime>`, and an `actions` block with buttons `Visit Uptime Kuma` (`<primaryBaseURL>/dashboard/<id>`) and `Visit site` (the address when it is a URL) (`:36-135`, `:196-200`).
- A set `slackbutton` is copied into setting `primaryBaseURL` if that is empty (`:17-28`, `:210-212`).

### teams

`server/notification-providers/teams.js`. Keys: `webhookUrl` (note the casing, unlike webhook's `webhookURL`), `teamsEnableTags` (default `false`, `:264`).

Payload (`:187-212`): `{type: "message", summary, attachments: [{contentType: "application/vnd.microsoft.card.adaptive", contentUrl: "", content: {type: "AdaptiveCard", body, $schema: "http://adaptivecards.io/schemas/adaptive-card.json", version: "1.5"}}]}`.

- `summary`: `🔴 [<name>] went down`, `✅ [<name>] is back online`, or `Notification` (`:16-23`).
- Body: a container with a column set (style `attention`, `good` or `emphasis`, `:30-38`), the Kuma logo and the bold title without the emoji, subtitle `Uptime Kuma Alert`; a fact set with `Description` (heartbeat msg), `Monitor`, `URL` as `[url](url)`, `Time` as `<localDateTime> (<timezone>)`; optional tag badges; an `ActionSet` with `Visit Uptime Kuma` and `Visit Monitor URL`, appended even when empty (`:62-212`).
- Test, certificate, domain: the same card with only `Description: msg`, title `Notification` (`:232-240`, `:249-252`).

### telegram

`server/notification-providers/telegram.js`. Keys: `telegramBotToken`, `telegramChatID`, `telegramServerUrl` (server default `https://api.telegram.org` via `??`, `:53`; frontend sets it when empty, `Telegram.vue:155-157`), `telegramMessageThreadID`, `telegramSendSilently`, `telegramProtectContent`, `telegramUseTemplate`, `telegramTemplate`, `telegramTemplateParseMode` (`plain`, `HTML`, `MarkdownV2`; `Telegram.vue:91-93`).

`POST <server>/bot<token>/sendMessage` with `{chat_id, text: msg, disable_notification, protect_content, link_preview_options: {is_disabled: true}}` plus `message_thread_id` when set (`:56-65`, `:104`). With a template, `text` is the rendered template and `parse_mode` is set unless the mode is `plain` (`:67-100`). For `MarkdownV2`, `msg` and every string in both JSON objects are escaped first; when `monitorJSON` is null an escaped stand-in `{name, hostname, url}` is used (`:71-88`).

### ntfy

`server/notification-providers/ntfy.js`. Keys: `ntfyserverurl`, `ntfytopic`, `ntfyPriority`, `ntfyPriorityDown`, `ntfyAuthenticationMethod` (`none`, `usernamePassword`, `accessToken`), `ntfyusername`, `ntfypassword`, `ntfyaccesstoken`, `ntfyCall`, `ntfyIcon`, `ntfyUseTemplate`, `ntfyCustomTitle`, `ntfyCustomMessage`. Frontend defaults: server `https://ntfy.sh` and priority `5` for new channels, `ntfyPriorityDown` `5`, auth method migrated from `ntfyusername` (`Ntfy.vue:168-194`).

- Auth header Basic or Bearer by method only; a legacy config with `ntfyusername` but no method sends no auth (`:15-26`). `X-Call` header when `ntfyCall` is set (`:27-29`).
- `POST ntfyserverurl` with JSON; the topic is in the body.
- Test, certificate, domain: `{topic, title: "<monitor name or topic> [Uptime-Kuma]", message: msg, priority: ntfyPriority, tags: ["test_tube"]}`, title and message templatable (`:35-61`).
- Up and down: priority `ntfyPriority || 4`; for DOWN `ntfyPriorityDown`, else priority plus one capped at 5; tags `red_circle` or `green_circle` plus monitor tags as `name` or `name: value`; title `<name> Down [Uptime-Kuma]` or `<name> Up [Uptime-Kuma]`; message `heartbeatJSON.msg`; `actions` with a `view` button `Open <name>` when the monitor has a URL; `icon` when set (`:63-129`).

### pushover

`server/notification-providers/pushover.js`. Keys: `pushoveruserkey`, `pushoverapptoken`, `pushoversounds`, `pushoversounds_up`, `pushoverpriority`, `pushovertitle`, `pushoverdevice`, `pushoverttl`.

`POST https://api.pushover.net/1/messages.json` with `{message: msg, user, token, sound, priority, title, retry: "30", expire: "3600", html: 1}` (`:16-28`), plus `url` and `url_title: "Link to Monitor"` when setting `primaryBaseURL` and `monitorJSON` exist (`:30-34`), `device` and `ttl` when set. Up and down append `\n<b>Time (<timezone>)</b>: <localDateTime>` to `message`; UP uses `pushoversounds_up` when set (`:50-56`). No templates.

## Import mapping

Per `notification` row:

| Kuma | V1Per |
| --- | --- |
| `config.type` in the core set | active channel of that provider on the target node |
| any other registered type | channel kept with sealed `config`, inactive, reason "provider not supported yet" |
| type not in the 98 (for example a removed provider) | channel kept with sealed `config`, inactive, reason "unknown provider type"; Kuma itself could not send it (`server/notification.js:241-242`) |
| `name` column | channel name (equals `config.name` after any save) |
| `is_default` column | default flag for new monitors in the subtree |
| `active` column | ignored for sending, Kuma never reads it; record it in the import inventory |
| `user_id` | resolved through the user mapping of decision 0007 |
| `monitor_notification` rows | links from monitors to the channel |
| `config.applyExisting`, `config.isDefault` | dropped from provider settings; `isDefault` is covered by the column |
| `notification_sent_history` rows (`type = 'certificate'`) | per-monitor certificate thresholds already alerted, so V1Per does not repeat them |

Imported email channels carry their own SMTP settings, so they never use the instance transport. The whole `config`, including passwords, tokens and webhook URLs, is sealed (decision 0006).

## V1Per obligations

Must match Kuma exactly for the same config and event:

- Event `msg` strings, including emoji, brackets, spacing and the certificate and domain sentences.
- Template context names, values and fallbacks, Liquid semantics of liquidjs 10.26.0, and which events and providers apply templates.
- Email: subject, body text, `text` vs `html` choice, recipients, extra headers, DKIM, TLS and STARTTLS settings.
- Discord: format selection including the legacy flag, payload keys and values, embed titles, colours, field names, order and optional rules, `<t:...:F>` timestamps, duration text, avatar fallback, thread and forum handling, suppress flag.
- Up and down trigger rules, first-beat suppression, resend counting and certificate threshold bookkeeping.

May differ:

- Delivery: Kuma sends once inline and only logs failures; V1Per uses a durable outbox with retries (decision 0005). The avatar check `GET` may be cached per webhook.
- MIME framing, `Message-ID`, `Date` and other headers nodemailer adds, and JSON whitespace.
- `monitorJSON` and `heartbeatJSON` field sets beyond the keys documented above. Templates and webhook bodies that read other Kuma monitor fields may render differently; the importer should flag templates that reference `monitorJSON.` keys outside that list.
- Error message wording from `throwGeneralAxiosError`.
- The `active` column: V1Per may honour its own active flag, since Kuma never sets the column.
- Domain expiry: Kuma sends it, decision 0005 does not list it. Open for Mike.

## Not verified

`node_modules` is not part of the pinned source, so these rest on library behaviour, not on read code:

- The exact `time` string from `R.isoDateTimeMillis` (redbean-node), expected `YYYY-MM-DD HH:mm:ss.SSS` in UTC, and how Discord parses it as an embed `timestamp`.
- liquidjs 10.26.0 defaults: no output escaping, so values in an HTML body are not escaped; how an object such as `{{ monitorJSON }}` stringifies.
- axios 0.32 content type for the string body of a `custom` webhook (expected `application/x-www-form-urlencoded` unless a header overrides it).
- nodemailer 7.0.13 handling of `secure: false` (opportunistic STARTTLS) and of the `headers` key in transport options.
- `server/client.js:26` reads `isDefault` from `bean.export()`; whether redbean-node exposes `is_default` under that name decides whether the UI list shows defaults. Sending is not affected.
