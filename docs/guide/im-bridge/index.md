# IM Bridge

The Rust control plane stores configuration and chat authorization state for Telegram, Slack, Discord, Feishu/Lark, DingTalk, WeCom, and Weixin.

## Status Semantics

| State | Meaning |
|---|---|
| configured | Required platform fields were accepted and stored in Rust Home |
| enabled | The group requested the bridge to run |
| running | A real network adapter is active |
| authorized | A chat passed the explicit pairing/subscription flow |

The Rust package does not report `running=true` merely because configuration exists. Platform network adapters must be available and validated independently.

## CLI

```bash
cccc im set <platform> [credential options] [--group ID]
cccc im config [--group ID]
cccc im start [--group ID]
cccc im stop [--group ID]
cccc im status [--group ID]
cccc im pending [--group ID]
cccc im bind --key KEY [--group ID]
cccc im authorized [--group ID]
cccc im reject --key KEY [--group ID]
cccc im revoke --chat-id ID [--thread-id N] [--group ID]
```

Secrets should be supplied through environment variable names rather than literal values. Configuration is isolated per group under `CCCC_HOME`.

## Chat Subscription

Send `/subscribe` to the bot. The bot immediately replies with the real 12-character pairing key, which expires after 10 minutes. Repeating `/subscribe` from the same chat and platform during that window returns the same pending key instead of creating duplicates.

Weixin is the exception: confirming the QR login automatically authorizes the scanning account for that group. It does not require `/subscribe` or a separate pending-request approval. Logging out removes only this QR-created authorization and preserves manually bound chats.

Approve the request from Web **Pending Requests**, or use:

```bash
cccc im pending --group GROUP_ID
cccc im bind --key KEY --group GROUP_ID
```

The bot also replies immediately to `/unsubscribe`, `/pause`, `/resume`, `/verbose [on|off]`, `/status`, `/help`, invalid `/send` usage, and unknown commands. `/verbose` is idempotent: no argument or `on` enables it, while `off` disables it.

## Inbound Attachments

Authorized chats can send attachment-only messages as well as text with attachments. The Rust adapters download provider-hosted files into the group's content-addressed `state/blobs/` storage and attach normalized metadata to the message before dispatching it to agents.

- Discord accepts message attachments.
- Telegram accepts documents, photos, videos, audio, voice messages, and video notes.
- Feishu/Lark accepts images, files, audio, video/media, stickers, and resources embedded in rich posts. The app needs the `im:resource` scope.
- DingTalk accepts pictures, files, audio, video, and images embedded in rich text. A valid RobotCode is required to exchange download codes for signed URLs.
- WeCom accepts image, file, voice, video, and mixed-message attachments, including encrypted downloads.
- Weixin accepts image, file, voice, and video media from the SDK's decrypted media path.
- Slack accepts private image and file downloads with bot authentication.

Each inbound attachment is limited to 10 MiB. Advertised oversize files are rejected before download where the provider supplies a size; streamed downloads are checked again while writing to blob storage. One failed attachment does not discard other successfully downloaded attachments or accompanying text.

Weixin outbound messages send the agent's text first, followed by every attached file or image. Attachment titles are preserved so the SDK can select the correct Weixin media type; each outbound attachment uses the same 10 MiB limit and must resolve inside the group's blob storage.

## Security

- One configuration belongs to one group.
- Pending chats require explicit approval.
- Revocation removes the chat from the authorized list.
- The daemon remains the collaboration source of truth; adapters are transport ports.
- Test both inbound and outbound delivery before using an IM platform for operational control.

The platform-specific pages in this directory describe provider-side bot creation. Treat any adapter-specific command in an older page as legacy until the Rust status endpoint reports a real worker.
