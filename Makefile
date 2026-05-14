HOST ?= 127.0.0.1
PORT ?= 44020

.PHONY: all
all: build

.PHONY: build
build:
	cargo build --all

.PHONY: test
test: build
	cargo test --all-targets

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: lint
lint:
	cargo clippy --version
	cargo clippy --all-targets -- -D warnings

.PHONY: run
run:
	cargo run --bin helloworld -- \
	--host $(HOST) \
	--port $(PORT)

out/helloworld/index.json: \
	Cargo.lock Cargo.toml rust-toolchain.toml $(shell find images/helloworld src -type f ! -path '*/target/*')
	$(call build,helloworld)

define build_context
$$( \
	mkdir -p out; \
	self=$(1); \
	for each in $$(find out/ -maxdepth 2 -name index.json); do \
    	package=$$(basename $$(dirname $${each})); \
    	if [ "$${package}" = "$${self}" ]; then continue; fi; \
    	printf -- ' --build-context %s=oci-layout://./out/%s' "$${package}" "$${package}"; \
	done; \
)
endef

,:=,
define build
	$(eval NAME := $(1))
	$(eval TYPE := $(if $(2),$(2),dir))
	$(eval REGISTRY := tkhq-tvc-helloworld)
	$(eval PLATFORM := linux/amd64)
	DOCKER_BUILDKIT=1 \
	SOURCE_DATE_EPOCH=1 \
	BUILDKIT_MULTIPLATFORM=1 \
	docker build \
		--build-arg VERSION=$(VERSION) \
		--tag $(REGISTRY)/$(NAME) \
		--progress=plain \
		--platform=$(PLATFORM) \
		--label "org.opencontainers.image.source=https://github.com/tkhq/tvc-helloworld" \
		$(if $(filter common,$(NAME)),,$(call build_context,$(1))) \
		$(if $(filter 1,$(NOCACHE)),--no-cache) \
		--output "\
			type=oci,\
			$(if $(filter dir,$(TYPE)),tar=false$(,)) \
			rewrite-timestamp=true,\
			force-compression=true,\
			name=$(NAME),\
			$(if $(filter tar,$(TYPE)),dest=$@") \
			$(if $(filter dir,$(TYPE)),dest=out/$(NAME)") \
		-f images/$(NAME)/Containerfile \
		.
endef
