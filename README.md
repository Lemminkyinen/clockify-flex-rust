# Clockify Flex Rust

Clockify Flex Rust is an API client to check general stats from Clockify. It fetches user data, working days, days off, and public holidays from the Clockify API and calculates various statistics.

## Features

- Fetch user data from Clockify API
- Calculate working days, days off, and public holidays
- Include today in calculations
- Optional start balance in minutes



## Usage

Requires clockify API token. Can be set in environment variables or as a command line argument with `-t`.

You can run the program using the following command:

```shell
TOKEN=your_clockify_api_token
cargo run -r
```

Alternatively, you can use the pre-built binary found in tags/releases:
```shell
./clockify-flex-rust [OPTIONS]
```
### Options
- `-i`, `--include-today`: Include today in calculations
- `-t`, `--token` <TOKEN>: Clockify API token
- `-s`, `--start-date` <START_DATE>: Start date equal or greater than 2023-01-01 in the format YYYY-MM-DD
- `-b`, `--start-balance` <START_BALANCE>: Optional start balance in minutes
- `-h`, `--help`: Print help

### Example
```sh
./clockify-flex-rust -t your_clockify_api_token -s 2023-06-01 -b 100 -i
```

## Build

Prerequisites:
- Rust and related development packages

To build the project, run:
```sh
cargo build --release
```
Run
To run the project, use:
```sh
cargo run -r
```

## Notes
Use at your own risk, might explode.

## License
This project is licensed under the MIT License.