.PHONY: demo install dev test rust-test diagnostic reset-lancedb clean-dev-cache clean-all-generated clean-dev-cache-dry-run

install:
	npm install

demo: install
	npm run tauri dev

test:
	npm run typecheck
	npm test
	# cargo test compiles the Tauri binary, whose generate_context!() requires
	# the frontend bundle (frontendDist = ../dist) to exist at compile time.
	npm run build
	cd src-tauri && cargo test

rust-test:
	test -d dist || npm run build
	cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test

diagnostic:
	./scripts/run_embedding_search_diagnostic.sh

reset-lancedb:
	./scripts/reset_lancedb.sh

clean-dev-cache:
	./scripts/clean-dev-build-cache.sh --yes

clean-all-generated:
	./scripts/clean-dev-build-cache.sh --yes --all

clean-dev-cache-dry-run:
	./scripts/clean-dev-build-cache.sh --dry-run
