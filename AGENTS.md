# AGENTS

You are building a bot using Rust, hosted on Vercel

## RULES

- find docs for your task in `.repos/`
- Rust AI SDK repo and docs `.repos/aisdk`
- Vercel Rust docs `.repos/VERCEL_RUST.md`

## CODING

- keep it simple, stupid
- design panic-free code
- design types to prevent illegal states
- structs for data
- enums for states
- avoid boolean flags
- avoid mutations
- use Rust functional features
- use Result/Option for error/absence
- unwrap Result/Option at IO boundaries
- use `?` for unwrapping Result/Option
- unwrap custom types on final usage
- match for branching and error handling
- never match using wildcards

## COMMIT MESSAGES

- short and imperative
- max length 5 words

Examples:

- dev(ci): add build matrix
- dev(test): refactor bot tests
- dev(chore): update dependencies
- feat(ai): add ai feature to bot
