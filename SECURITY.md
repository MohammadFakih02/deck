# Security

## Supported versions

Only the latest release gets fixes. Update from the [Releases](https://github.com/MohammadFakih02/deck/releases) page. Each setup has a SHA-256 checksum next to it. Deck can update itself from these releases (Settings → About, on by default): it only takes a newer plain `X.Y.Z` release of this repository, checks the download against the release's `.sha256` and runs it silently only when Deck already has admin rights and sits in a folder only admins can change. Otherwise it asks once (UAC) and a Deck with admin rights copies the setup into that admin-only folder and checks it again before it runs; a setup is never run with admin rights from a folder the user can change. The checksum comes from the same release as the setup, so it catches a damaged download, not a hijacked release; the setup is not code-signed.

## Reporting a problem

Please don't open a public issue for a security problem. Use GitHub's private reporting instead: the **Security** tab of this repository → **Report a vulnerability**. If that isn't available to you, open an issue that only asks for a private contact, with no details.

Helpful to include:
- the Deck version;
- whether Deck was running as admin (after **Set up**) or not;
- the steps to reproduce;
- what an attacker would need first: a program running as the signed-in user, a device on the same network, the phone key, the pairing code, or something else.

Fixes are listed under **Security** in [`CHANGELOG.md`](CHANGELOG.md).

## What counts

Deck runs with administrator rights, listens on the home network, and controls hardware. These are in scope:

- **Admin rights from a normal account.** Any way for a program running as the signed-in user, with no admin rights, to make Deck do something only an administrator could do.
- **Reaching Deck over the network without the right secret.** Any way for a device on the network to use the phone remote, PC to PC, Claude Code sync or a share link without that secret, or to do more than that caller is allowed to.
- **Escaping the folders Deck may use.** Any way to make Deck read, write or delete files outside the folders a feature is meant to use.
- **Hardware left unsafe.** Any way to leave the hardware in an unsafe state that Deck's guards don't catch.

[`docs/trust-boundaries.md`](docs/trust-boundaries.md) describes each boundary, where it is enforced, and the risks that are known and accepted (for example, phone screen control can reach admin windows). Those accepted risks aren't new findings, but ideas for reducing them are welcome.

Out of scope:
- **Physical access**, or anything that already requires admin rights.
- **Overclocking settings the user chose.** Deck's guards limit them, but can't make every setting safe.
- **Third-party tools Deck installs or drives** (RivaTuner, RustDesk, Tailscale, winget). Report those to their makers.
