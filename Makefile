build:
	@cargo build

test:
	@cargo nextest run --all-features

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

check:
	@cargo clippy --all-targets --all-features --tests --benches -- -D warnings

.PHONY: build test release update-submodule check
