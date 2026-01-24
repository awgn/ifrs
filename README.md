# ifrs

A command-line tool to display detailed information about network interfaces on Linux and macOS systems.

## Description

`ifrs` (Interface Information Tool) provides a comprehensive view of network interfaces, including IP addresses, MAC addresses, driver details, PCI information, and more. It supports filtering and fuzzy searching to quickly find specific interfaces.

## Installation

### From Source

Ensure you have Rust installed. Then clone the repository and build:

```bash
git clone https://github.com/awgn/ifrs.git
cd ifrs
cargo build --release
```

The binary will be available at `target/release/ifrs`.

### Using Cargo Install

If you have Rust installed, you can install directly from crates.io:

```bash
cargo install ifrs
```

## Usage

```bash
ifrs [OPTIONS] [KEYWORDS]...
```

### Options

- `-a, --all`: Display all interfaces, even if they are down.
- `-v, --verbose`: Enable verbose output, showing additional details like features, rings, and channels (Linux only).
- `-4, --ipv4`: Show only interfaces with IPv4 addresses.
- `-6, --ipv6`: Show only interfaces with IPv6 addresses.
- `-r, --running`: Show only running interfaces (link detected).
- `-i, --ignore-case`: Perform case-insensitive matching for keywords.
- `-h, --help`: Print help information.
- `-V, --version`: Print version information.

### Keywords and Fuzzy Search

You can provide one or more keywords as trailing arguments. The tool performs a fuzzy search, meaning it checks if any of the keywords are substrings of various interface attributes. The search is case-sensitive by default, but can be made case-insensitive with the `-i` flag.

The following attributes are searched:

- Interface name
- Flags (e.g., "UP", "BROADCAST")
- Media type (e.g., "Ethernet", "Wireless")
- MAC address
- IPv4 addresses
- IPv6 addresses
- Driver name
- Driver version
- Bus info
- PCI address
- Vendor name
- Device name

If any keyword matches any of these attributes, the interface is displayed. If no keywords are provided, all matching interfaces (based on other filters) are shown.

### Examples

1. **List all interfaces:**
   ```bash
   ifrs
   ```
   Shows only up interfaces with link detected by default.

2. **List all interfaces, including down ones:**
   ```bash
   ifrs -a
   ```

3. **Show only running interfaces:**
   ```bash
   ifrs -r
   ```

4. **Filter by interface name:**
   ```bash
   ifrs eth0
   ```
   Shows interfaces whose name contains "eth0".

5. **Case-insensitive search for wireless interfaces:**
   ```bash
   ifrs -i wifi
   ```
   Matches interfaces with "wifi" in any attribute, ignoring case.

6. **Show only interfaces with IPv4 addresses:**
   ```bash
   ifrs -4
   ```

7. **Verbose output for all interfaces:**
   ```bash
   ifrs -a -v
   ```

8. **Search for interfaces with a specific MAC address prefix:**
   ```bash
   ifrs 00:1b:44
   ```

9. **Combine filters:**
   ```bash
   ifrs -r -i ethernet
   ```
   Shows running interfaces with "ethernet" in any attribute (case-insensitive).

10. **Search for interfaces with a specific driver:**
    ```bash
    ifrs ixgbe
    ```
    Shows interfaces whose driver name contains "ixgbe".

## Output Format

Each interface is displayed with its name, status ([link-up] or [link-down]), and optional namespace. Then, indented details include:

- MAC address
- IPv4 and IPv6 addresses
- Flags
- Driver information
- PCI details
- MTU and metric
- Media type
- Statistics (RX/TX bytes and packets)
- Verbose: Features, rings, channels (Linux only)

## Platform Support

- **Linux**: Full support with ethtool integration for advanced features.
- **macOS**: Basic support via system calls.

## License

MIT OR Apache-2.0
