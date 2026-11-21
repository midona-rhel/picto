# Sourced by the steps of `.github/workflows/gpui-experiment.yml` that start
# the built programs directly (`. ci/env.sh`, from `experiments/gpui`).
#
# It defines two functions that turn a path into the form a program on this
# system expects, and it puts the folders that hold the ONNX Runtime
# libraries on the loader's search path. The application core loads those
# libraries when it starts, and the `ort` crate copies them into
# `target/debug` and `target/debug/deps`. `cargo run` and `cargo test` add
# these folders by themselves; a program started directly needs them given.
#
# The search path is set here, for one step at a time, and not for the whole
# job. A build script of the `ort` crate runs again when the search path
# differs from the one it last saw, and Cargo then compiles the application
# core and the app again, which takes several minutes. So every step that
# calls Cargo must see the same environment, without these folders.

# Prints the path `$1` in the system's own form: with a drive letter and
# backslashes on Windows, unchanged elsewhere.
native() { if [[ "$RUNNER_OS" == Windows ]]; then cygpath -w "$1"; else printf '%s' "$1"; fi; }

# Prints the path `$1` in the form the shell uses: `/d/a/…` on Windows,
# unchanged elsewhere.
unix() { if [[ "$RUNNER_OS" == Windows ]]; then cygpath -u "$1"; else printf '%s' "$1"; fi; }

libraries="$PWD/target/debug:$PWD/target/debug/deps"
if [[ "$RUNNER_OS" == Windows ]]; then
  # Windows looks for a program's DLLs in the folders of `PATH`.
  export PATH="$libraries:$PATH"
else
  export LD_LIBRARY_PATH="$libraries${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
