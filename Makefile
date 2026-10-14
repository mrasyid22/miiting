CARGO := PATH="/opt/homebrew/opt/rustup/bin:$$HOME/.cargo/bin:$$PATH" cargo
CLI := core/target/release/miiting-cli
MODELS := $(HOME)/Library/Application Support/Miiting/models

.PHONY: help core app run install dmg test lint cli demo models fleurs bench e2e clean

help:
	@echo "make app      Build Miiting.app (Rust core + SwiftUI) into build/"
	@echo "make run      Build, then open the app"
	@echo "make install  Copy Miiting.app to /Applications"
	@echo "make dmg      Package build/Miiting.app as build/Miiting-<version>.dmg"
	@echo "make test     Run the Rust core unit tests"
	@echo "make lint     cargo fmt --check, clippy, localization check"
	@echo "make demo     Open the app with sample meetings (your data is untouched)"
	@echo "make models   Download the recommended models (~3.5 GB, asks first)"
	@echo "make fleurs   Download Indonesian FLEURS for benchmarks (~430 MB, asks first)"
	@echo "make bench    WER/CER/RTF on Indonesian FLEURS"
	@echo "make e2e      End-to-end test: synthetic meeting → live → final → notes (Ollama)"

core:
	scripts/build-core.sh

app:
	scripts/build-app.sh

run: app
	open build/Miiting.app

install: app
	rm -rf /Applications/Miiting.app && cp -R build/Miiting.app /Applications/
	@echo "✓ /Applications/Miiting.app"

dmg: app
	scripts/make-dmg.sh build/Miiting.app

test:
	$(CARGO) test --manifest-path core/Cargo.toml -p miiting-core

lint:
	$(CARGO) fmt --manifest-path core/Cargo.toml --all -- --check
	$(CARGO) clippy --manifest-path core/Cargo.toml --workspace --all-targets -- -D warnings
	scripts/check-localization.py

cli:
	$(CARGO) build --manifest-path core/Cargo.toml --release -p miiting-cli

demo: cli
	rm -rf build/demo-data && $(CLI) seed-demo --data-dir build/demo-data
	open -n --env MIITING_DATA_DIR="$(CURDIR)/build/demo-data" --env MIITING_SKIP_ONBOARDING=1 \
		--env MIITING_SELECT=demo-sprint build/Miiting.app

models:
	scripts/fetch-models.sh

fleurs:
	scripts/fetch-fleurs.sh

# Override: make bench BENCH_MODELS="large-v3 large-v3-turbo-q5_0" BEAMS=1,5 LIMIT=200
BENCH_MODELS ?= large-v3-turbo-q5_0 large-v3
BEAMS ?= 1,5
LIMIT ?= 300
bench: cli
	$(CLI) bench --fleurs data/fleurs/id_id --limit $(LIMIT) --beam $(BEAMS) \
		$(foreach m,$(BENCH_MODELS),--model "$(MODELS)/ggml-$(m).bin") \
		--out reports/fleurs-$(shell date +%Y%m%d-%H%M)

# Synthesizes the test meeting if needed, then streams it through the engine like the app.
E2E_LIVE ?= large-v3-turbo-q5_0
E2E_FINAL ?= large-v3
E2E_LLM ?= gemma3:12b
e2e: cli
	@[ -f data/test-meeting/mic.wav ] || scripts/make-test-meeting.sh
	rm -rf build/e2e
	$(CLI) simulate --mic data/test-meeting/mic.wav --system data/test-meeting/system.wav \
		--data-dir build/e2e --speed 4 --reference data/test-meeting/reference.txt \
		--live-model "$(MODELS)/ggml-$(E2E_LIVE).bin" --final-model "$(MODELS)/ggml-$(E2E_FINAL).bin" \
		--vad "$(MODELS)/ggml-silero-v5.1.2.bin" \
		$(shell curl -s -m 2 localhost:11434/api/tags >/dev/null && echo --ollama-model $(E2E_LLM))

clean:
	rm -rf build apps/macos/.build apps/macos/lib
	$(CARGO) clean --manifest-path core/Cargo.toml
