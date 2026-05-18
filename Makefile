DATABASE_URL      ?= postgres://odnelazm:odnelazm@localhost:5432/odnelazm
CLOUD_DATABASE_URL ?=
METRICS_URL       ?=
MODEL        ?= google/gemma-4-e4b
CONCURRENCY  ?= 1
BATCH        ?= 10
START_DATE   ?= 2022-09-01
END_DATE     ?= $(shell date +%Y-%m-%d)
PARLIAMENT   ?= 13th-parliament

PIPELINE = ./target/release/odnelazm-pipeline

PIPELINE_FLAGS = --database-url $(DATABASE_URL) $(if $(METRICS_URL),--metrics-url $(METRICS_URL),)

.PHONY: build ingest ingest-with-profiles enrich-bill-mentions enrich-bill-journeys \
        enrich-bill-speakers enrich-topics enrich-topic-speakers enrich-sittings \
        enrich-all db-pull db-push metrics-up metrics-down metrics-logs

build:
	cargo build -p odnelazm-ingest --release

## Ingestion

ingest: build
	$(PIPELINE) $(PIPELINE_FLAGS) ingest \
		--start-date $(START_DATE) \
		--end-date $(END_DATE) \
		--parliament $(PARLIAMENT)

ingest-with-profiles: build
	$(PIPELINE) $(PIPELINE_FLAGS) ingest \
		--start-date $(START_DATE) \
		--end-date $(END_DATE) \
		--parliament $(PARLIAMENT) \
		--import-profiles

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

enrich-all: enrich-bill-mentions enrich-bill-journeys \
            enrich-bill-speakers enrich-topics enrich-topic-speakers enrich-sittings

## DB sync

db-pull:
	@if [ -z "$(CLOUD_DATABASE_URL)" ]; then echo "CLOUD_DATABASE_URL is not set"; exit 1; fi
	@echo "Dumping schema from cloud..."
	docker run --rm --network host postgres:17-alpine \
	  pg_dump "$(CLOUD_DATABASE_URL)" --no-owner --no-acl --schema-only \
	  > /tmp/odnelazm_cloud_schema.sql
	@echo "Dumping data from cloud..."
	docker run --rm --network host postgres:17-alpine \
	  pg_dump "$(CLOUD_DATABASE_URL)" --no-owner --no-acl --data-only \
	  > /tmp/odnelazm_cloud_data.sql
	@echo "Resetting local DB..."
	docker exec odnelazm-pg psql -U odnelazm -d odnelazm \
	  -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
	@echo "Applying schema..."
	docker exec -i odnelazm-pg psql -U odnelazm -d odnelazm < /tmp/odnelazm_cloud_schema.sql
	@echo "Applying data..."
	docker exec -i odnelazm-pg psql -U odnelazm -d odnelazm < /tmp/odnelazm_cloud_data.sql
	@echo "Done."

db-push:
	@if [ -z "$(CLOUD_DATABASE_URL)" ]; then echo "CLOUD_DATABASE_URL is not set"; exit 1; fi
	@echo "Dumping schema from local..."
	docker exec odnelazm-pg pg_dump -U odnelazm -d odnelazm --no-owner --no-acl --schema-only \
	  -f /tmp/odnelazm_local_schema.sql
	@echo "Dumping data from local..."
	docker exec odnelazm-pg pg_dump -U odnelazm -d odnelazm --no-owner --no-acl --data-only \
	  -f /tmp/odnelazm_local_data.sql
	@echo "Resetting cloud DB..."
	docker run --rm --network host postgres:17-alpine \
	  psql "$(CLOUD_DATABASE_URL)" -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" \
	  -c "CREATE OR REPLACE FUNCTION public.uuid_generate_v4() RETURNS uuid LANGUAGE sql AS \$\$SELECT gen_random_uuid();\$\$;"
	@echo "Applying schema..."
	docker run --rm --network host -v /tmp:/tmp postgres:17-alpine \
	  psql "$(CLOUD_DATABASE_URL)" -f /tmp/odnelazm_local_schema.sql
	@echo "Applying data..."
	docker run --rm --network host -v /tmp:/tmp postgres:17-alpine \
	  psql "$(CLOUD_DATABASE_URL)" -f /tmp/odnelazm_local_data.sql
	@echo "Done."

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
