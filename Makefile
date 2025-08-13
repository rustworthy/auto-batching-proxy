APP_PORT=8081
INFERENCE_SERVICE_HOST=127.0.0.1
INFERENCE_SERVICE_PORT=8080
INFERENCE_SERVICE_MODEL_ID=nomic-ai/nomic-embed-text-v1.5
INFERENCE_SERVICE_CONTAINER_NAME=inference-service

# --------------------------- AUXILIARY BINARIES -------------------------------
.PHONY: tools/install
tools/install:
	cargo install cargo-watch
	cargo install bunyan
	cargo install oha

.PHONY: tools/uninstall
tools/uninstall:
	@cargo uninstall cargo-watch > /dev/null 2>&1 || true
	@cargo uninstall bunyan > /dev/null 2>&1 || true
	@cargo uninstall oha > /dev/null 2>&1 || true

# ------------------------- INFERENCE SERIVCE COMMANDS -------------------------
.PHONY: inference/start
inference/start:
	docker ps -a | grep -i ${INFERENCE_SERVICE_CONTAINER_NAME} && \
	docker start ${INFERENCE_SERVICE_CONTAINER_NAME} || \
	docker run -d --pull always \
	-p ${INFERENCE_SERVICE_HOST}:${INFERENCE_SERVICE_PORT}:80 \
	--name ${INFERENCE_SERVICE_CONTAINER_NAME} \
	ghcr.io/huggingface/text-embeddings-inference:cpu-latest --model-id ${INFERENCE_SERVICE_MODEL_ID}

.PHONY: inference/stop
inference/stop:
	@docker ps | grep -i ${INFERENCE_SERVICE_CONTAINER_NAME} && \
	docker stop ${INFERENCE_SERVICE_CONTAINER_NAME} || true

.PHONY: inference/rm
inference/rm: inference/stop
	docker rm ${INFERENCE_SERVICE_CONTAINER_NAME} || true

# ---------------------------- SETUP COMMANDS ----------------------------------
.PHONY: dotenv
dotenv:
	@echo "🔑 Setting up dotenv file"
	@echo "" >> .env
	@echo "# The content below has been copied from .env.example file" >> .env
	@cat .env.example | tee -a .env

.PHONY: setup
setup: dotenv tools/install inference/start

# ------------------------- DEVELOPMENT COMMANDS -------------------------------
PHONY: watch
watch: inference/start
	fuser -k ${APP_PORT}/tcp || true
	cargo watch --clear --exec "run" --no-dot-ignores

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: check
check:
	cargo fmt --check
	cargo clippy --all-features --all-targets
	cargo doc --no-deps --all-features

# ------------------------- TESTING COMMANDS -------------------------------
.phony: load
load:
	oha -c 200 -z $(duration) --latency-correction \
		-m POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json' \
		http://localhost:${APP_PORT}/embed

.phony: load/noproxy
load/noproxy:
	oha -c 200 -z $(duration) --latency-correction \
		-m POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json' \
		http://localhost:${INFERENCE_SERVICE_PORT}/embed

