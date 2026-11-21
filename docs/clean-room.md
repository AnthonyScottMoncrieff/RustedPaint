# Clean-room policy

RustedPaint is independently implemented. The Paint.NET research bundle is a
behavioural reference: it may supply facts to verify, input fixtures with clear
licensing, and independently written acceptance criteria.

It must not be treated as source code. Do not copy its Rust implementation,
internal API shapes, identifiers, or text into this project. When behaviour is
important, record the observable contract in a RustedPaint test and implement
the smallest design that satisfies that test.

Keep these categories separate in issues and commits:

- **Observed behaviour:** a user-visible result with a reproducible fixture.
- **RustedPaint design:** an independently chosen API or implementation.
- **Compatibility target:** an intentionally supported interchange format or
  rendering outcome.

The project starts with its own document types and test suite; it does not use
or depend on the reference bundle's code.
