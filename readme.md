# Agave Repo Function Tester

This is a demonstration of how to extract and test specific functions or code snippets from the Anza Agave repository without building the entire project. This approach allows for focused testing and experimentation with individual components.

## Project Context

This project is part of the Turbin3 Advanced SVM cohort (Q1 2025) activity, aimed at improving developer skills in working with complex blockchain codebases such as Validator clients 

## Getting Started

Follow these steps to set up and use this testing environment:

1. Clone the Agave repository:
   ```
   git clone https://github.com/anza-xyz/agave.git
   cd agave
   ```

2. Copy the test files:
   - From this repo: `src/replay_stage_test.rs`
   - To Agave: `agave/core/src/replay_stage_test.rs`

3. Identify the package name:
   - Open `agave/core/Cargo.toml`
   - Look for the `package.name` field (e.g., "solana-core")

4. Build the specific package:
   ```
   cargo build -p solana-core
   ```

## Example

In this example, we're working with a function from `agave/core/src/replay_stage.rs`. The corresponding package is `solana-core`, as defined in `agave/core/Cargo.toml`.

## Usage

- No need to build the whole agave repo to test changes
- Isolated testing environment

## Contributing

Contributions to improve this testing approach or add more examples are welcome. Please submit a pull request or open an issue for discussion.

## License

This project is licensed under [INSERT LICENSE HERE]. Please see the LICENSE file for details.