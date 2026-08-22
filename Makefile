# Disc reverse-engineering pipeline.
.PHONY: oracle oracle-check clean
oracle:
	$(MAKE) -C oracle
oracle-check: oracle
	./scripts/oracle_check.sh
clean:
	$(MAKE) -C oracle clean
