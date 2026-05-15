# ffdump

Extracts direct download links from a FitGirl PrivateBin paste page.

Paste a URL in, get a ready-to-use `.txt` file saved to your Documents folder with all the `dl.fuckingfast.co` links in part order.

---

## Download

Go to the [Releases](../../releases/latest) page and grab the file for your platform:

| Platform | File |
|---|---|
| Windows (x86_64) | `ffdump-windows-x86_64.zip` |
| macOS (M1/M2/M3) | `ffdump-mac-arm64.tar.gz` |
| Linux (x86_64, any distro) | `ffdump-linux-x86_64.tar.gz` |

Extract the downloaded archive to get the `ffdump` executable.

---

## Usage

### Windows
Extract the `.zip` file, open a terminal in that folder, then run:
```cmd
.\ffdump.exe "https://paste.fitgirl-repacks.site/?abc123#YourKey"
```

### macOS / Linux
Extract the `.tar.gz` file, open a terminal in that folder, then run:
```bash
./ffdump "https://paste.fitgirl-repacks.site/?abc123#YourKey"
```

> **Important:** always wrap the URL in double quotes — the `#` key will be silently stripped by your terminal otherwise.

The links are saved to `Documents/<GameName>_direct_fuckingfast_links.txt`, sorted part001 → part002 → ...

---

## Options

```
ffdump <paste-url> [options]

  --concurrency N    How many links to fetch at once (default: 15)
  --password P       Paste password, if the paste is protected
  --output FILE      Custom output path instead of ~/Documents/
  --verbose          Show which file maps to which download link
```

---

## Requirements

- No installation needed — it's a single file
- No browser or Chrome required
- Internet connection
