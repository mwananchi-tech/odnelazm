DATABASE_URL ?= postgres://odnelazm:odnelazm@localhost:5432/odnelazm
METRICS_URL  ?=
MODEL        ?= google/gemma-4-e4b
CONCURRENCY  ?= 1
BATCH        ?= 10
START_DATE   ?= 2022-09-01
END_DATE     ?= $(shell date +%Y-%m-%d)
PARLIAMENT   ?= 13th-parliament

PIPELINE = ./target/release/odnelazm-pipeline

PIPELINE_FLAGS = --database-url $(DATABASE_URL) $(if $(METRICS_URL),--metrics-url $(METRICS_URL),)

.PHONY: build ingest ingest-members enrich-bill-mentions enrich-bill-journeys \
        enrich-bill-speakers enrich-topics enrich-topic-speakers enrich-sittings \
        enrich-all metrics-up metrics-down metrics-logs

build:
	cargo build -p odnelazm-ingest --release

## Ingestion

ingest: build
	$(PIPELINE) $(PIPELINE_FLAGS) ingest \
		--start-date $(START_DATE) \
		--end-date $(END_DATE) \
		--parliament $(PARLIAMENT)

ingest-members: build
	$(PIPELINE) $(PIPELINE_FLAGS) ingest \
		--start-date $(START_DATE) \
		--end-date $(END_DATE) \
		--parliament $(PARLIAMENT) \
		--enrich-members

## Enrichment

enrich-bill-mentions: build
	$(PIPELINE) $(PIPELINE_FLAGS) enrich bill-mentions \
		--model $(MODEL) \
		--concurrency $(CONCURRENCY) \
		--batch $(BATCH)

enrich-bill-journeys: build
	$(PIPELINE) $(PIPELINE_FLAGS) enrich bill-journeys \
		--model $(MODEL) \
		--concurrency $(CONCURRENCY) \
		--batch $(BATCH)

enrich-bill-speakers: build
	$(PIPELINE) $(PIPELINE_FLAGS) enrich bill-speakers \
		--model $(MODEL) \
		--concurrency $(CONCURRENCY) \
		--batch $(BATCH)

enrich-topics: build
	$(PIPELINE) $(PIPELINE_FLAGS) enrich topics \
		--model $(MODEL) \
		--concurrency $(CONCURRENCY) \
		--batch $(BATCH)

enrich-topic-speakers: build
	$(PIPELINE) $(PIPELINE_FLAGS) enrich topic-speakers \
		--model $(MODEL) \
		--concurrency $(CONCURRENCY) \
		--batch $(BATCH)

enrich-sittings: build
	$(PIPELINE) $(PIPELINE_FLAGS) enrich sittings \
		--model $(MODEL) \
		--concurrency $(CONCURRENCY) \
		--batch $(BATCH)

enrich-all: enrich-bill-mentions enrich-bill-journeys enrich-bill-speakers \
            enrich-topics enrich-topic-speakers enrich-sittings

## Metrics stack

metrics-up:
	docker compose -f docker-compose.metrics.yml up -d
	@echo "Pushgateway: http://localhost:9091"
	@echo "Prometheus:  http://localhost:9090"
	@echo "Grafana:     http://localhost:3001"

metrics-down:
	docker compose -f docker-compose.metrics.yml down

metrics-logs:
	docker compose -f docker-compose.metrics.yml logs -f
