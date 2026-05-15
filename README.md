# ffdump

Extracts direct download links from a FitGirl PrivateBin paste page.

Paste a URL in, get a ready-to-use `.txt` file saved to your Documents folder with all the `dl.fuckingfast.co` links in part order.

---

## Download

Go to the [Releases](../../releases/latest) page and grab the file for your platform:

| Platform | File |
|---|---|
| Windows | `ffdump-windows.exe` |
| macOS (M1/M2/M3) | `ffdump-mac-arm64` |
| macOS (older Intel) | `ffdump-mac-intel` |
| Linux (any distro, x86_64) | `ffdump-linux` |

---

## Usage

### Windows
Open a terminal in the folder where you saved the `.exe`, then:
```
ffdump-windows.exe "https://paste.fitgirl-repacks.site/?abc123#YourKey"
```

### macOS / Linux
Make it executable once, then run it:
```bash
chmod +x ffdump-mac-arm64
./ffdump-mac-arm64 "https://paste.fitgirl-repacks.site/?abc123#YourKey"
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
