# Notifications

A notification channel sends alerts to email, chat or your own endpoint. Channels belong to a node and can be used by monitors on that node and every node below it. Creating and editing channels needs the Admin role on the node.

## What sends an alert

| Event | When |
| --- | --- |
| Down | A monitor goes from up, pending or maintenance to down, or its first check is down |
| Up | A down monitor recovers |
| Certificate | An http monitor's TLS certificate is about to expire |
| Probe offline | A remote probe has had no contact for 90 seconds |
| Test | You press the test button |

- A monitor alerts the channels linked to it. A new monitor where you pick no channels gets every **Default channel** on its node and the nodes above.
- **Repeat the down alert after** on a monitor sends the down alert again after that many further down checks. 0 sends it once.
- Down and up alerts of a monitor under maintenance are not sent. They show as Suppressed in the delivery log.
- Certificate alerts are turned on per monitor with **Alert before the certificate expires**. The days come from the instance setting **Certificate warnings**, 7, 14 and 21 by default. They do nothing while the monitor accepts invalid certificates.
- Probe offline alerts go to every active channel on the probe's own node.
- Incidents and maintenance windows do not send alerts themselves.

## Create a channel

Open the node's Channels page and add a channel. Every channel has:

- **Name**, 1 to 100 characters.
- **Default channel**: new monitors on this node and below get it when nobody picks channels.
- **Add to existing monitors**: links the channel to every monitor on the node and below when you save.
- **Active**: an inactive channel sends nothing.

Secret fields, such as passwords, tokens and webhook URLs, are stored encrypted. The form shows only whether one is set. Leave a secret field empty to keep the stored value.

URLs must start with `http://` or `https://`. Templates use Liquid, and a template that does not parse is refused when you save. Empty templates use the same messages as Uptime Kuma, such as `[API] [🔴 Down] timeout`.

## Providers

### Email (SMTP)

By default an email channel sends through the instance mail server, set by an instance admin in the instance settings. Turn on **Use its own mail server** to send through another one.

- **To**, **CC**, **BCC**: comma-separated addresses. Fill in at least one.
- **From**: empty uses the instance sender. Required when the channel has its own server.
- **SMTP host**, **Port**, **Security**: STARTTLS usually runs on port 587 or 25, TLS on port 465. **Never upgrade to TLS** sends in plain text. **Accept invalid certificates** skips the certificate check.
- **Username**, **Password**.
- **Subject template**, **Body template**, **Send the body as HTML** (custom bodies only).
- Under **Headers and DKIM signing**: extra headers as JSON, and DKIM domain, key selector, private key, hash algorithm and the headers to sign or skip.

The instance mail server has its own **Send test email** button.

### Discord

- **Webhook URL** from the Discord channel's integrations.
- **Display name**, empty for "Uptime Kuma".
- **Post to**: the channel, a new forum post with a **Post title**, or an existing thread by **Thread ID**.
- **Message format**: Normal, Minimalist or a custom template, with an optional **Prefix message**.
- **Hide the monitor's address** and **Post silently**.

### Slack

- **Webhook URL** of a Slack incoming webhook.
- **Channel**, **Username** and **Icon emoji**, to override the webhook's own.
- **Notify the whole channel**, **Rich message** and **Include the group name**.
- Optionally a custom template.

### Microsoft Teams

- **Webhook URL** of the Teams workflow or connector.
- **Mention tags**, to mention people or tags.

### Telegram

- **Bot token** from BotFather and the **Chat ID**. **Topic ID** for a topic in a group.
- **Server URL**, `https://api.telegram.org` by default.
- **Send silently**, **Protect from forwarding and saving**.
- **Formatting**: plain text, HTML or MarkdownV2, with an optional template.

### ntfy

- **Server URL**, for example `https://ntfy.sh` or your own server, and the **Topic**.
- **Priority** and **Priority when down**, 1 to 5.
- **Authentication**: none, username and password, or an access token.
- **Icon URL**, **Phone call**, and title and message templates.

### Pushover

- **User key** and **Application token**.
- **Device**, **Title**, **Sound when down** and **Sound when up**.
- **Priority**, -2 to 2, and **Time to live in seconds**.

### Webhook

- **URL**, and **Method**: POST or GET.
- **Body**: JSON, form data or a custom template.
- **Extra headers**: a JSON object, such as `{"Authorization": "Bearer ..."}`. A header here replaces a computed header of the same name.

With POST and JSON, V1Per sends `Content-Type: application/json` and `User-Agent: v1per/<version>`:

```json
{
  "heartbeat": {
    "monitorID": 42,
    "status": 0,
    "time": "2026-10-19 08:15:02.120",
    "msg": "Request failed with status code 503",
    "ping": null,
    "important": true,
    "duration": 60,
    "retries": 2,
    "response": null,
    "timezone": "Europe/Vienna",
    "timezoneOffset": "+02:00",
    "localDateTime": "2026-10-19 10:15:02"
  },
  "monitor": {
    "id": 42,
    "name": "Shop API",
    "path": ["Acme", "Shop API"],
    "pathName": "Acme / Shop API",
    "url": "https://shop.example.com/health",
    "hostname": null,
    "port": null,
    "type": "http",
    "subtype": null,
    "tags": [],
    "includeSensitiveData": false
  },
  "msg": "[Shop API] [🔴 Down] Request failed with status code 503"
}
```

The shape follows Uptime Kuma's webhook, so receivers built for Kuma keep working. `status` is 0 for down and 1 for up. `time` is UTC. Up alerts add `lastDownTime`. Test, certificate and probe offline messages send `heartbeat` and `monitor` as `null` with only `msg` set.

The other body types:

- **Form data**: a multipart POST with one field, `data`, holding the same JSON.
- **Custom template**: a POST of the rendered template with `Content-Type: application/x-www-form-urlencoded`. Set your own content type under extra headers.
- **GET**: query parameters `msg`, and `heartbeat` and `monitor` as JSON strings when set.

Any 2xx answer counts as delivered. Requests are not signed, so put a shared secret in a header or the URL if your receiver needs one.

## Test a channel

The **Test** panel sends one message right away, from the channel editor with the form as it is, or from the channel page with the saved settings.

1. Under **What to send**, pick a test message, a down alert or an up alert.
2. For down and up alerts, pick a **Monitor**: an example monitor or a real one on the node.
3. Press **Send test**.

On success you see "Test message sent." with what the provider answered, such as `HTTP 204`. On failure you see "The test message was not delivered." and the error. Tests are tried once and ignore maintenance.

## Delivery log

The channel page has a **Delivery log**, newest first. A monitor's page shows **Recent alerts** from its channels.

| Column | Shows |
| --- | --- |
| State | Pending, Delivered, Failed or Suppressed |
| Event | Down, Up, Certificate, Probe offline or Test |
| Message | The alert text, with the error in red or the provider's answer |
| Attempts | How many tries so far |
| When | When it was queued. Pending rows show the next try |

Temporary failures are retried: timeouts, connection errors, HTTP 408, 425, 429 and 5xx, and temporary SMTP errors. Retries start after 10 seconds and double up to 15 minutes between tries. After 24 hours the delivery fails for good. Other errors, such as a wrong token answered with 401, fail at once.

Alerts for one channel go out in order, so an up never arrives before the down it follows. Finished entries are kept for 30 days. Deleting a channel deletes its log.

## Channels imported from Uptime Kuma

Imported channels of providers V1Per supports work as before. Channels of other providers are imported inactive with the reason, such as "provider not supported yet". They cannot send, and you can delete them.
