# Agave Repo Function Tester

This is a demonstration of how to extract and test specific functions or code snippets from the Anza Agave repository without building the entire project. This approach allows for quick testing and experimentation with individual components with minimum dependencies to deal with.

## Project Context

[Turbin3](https://turbin3.com) Advanced SVM cohort (Q1 2025) project to deep dive into Solana Validator clients. 

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

3. Add to lib.rs
   ```
   pub mod replay_stage_test;
   ```

4. Identify the package name:
   - Open `agave/core/Cargo.toml`
   - Look for the `package.name` field (e.g., "solana-core")

5. Build the specific package:
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

Apache License 2.0

Copyright [2024] [farawaystar]

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0
Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
