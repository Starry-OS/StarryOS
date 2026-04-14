# Build Options
M ?=
DEFAULT_MODULE_PATHS := $(PWD)/modules

ifneq ($(M),)
DEFAULT_MODULE_PATHS := $(M)
endif

# Get list of modules, if M is specified:
# 1. If M is a directory and contains the Cargo.toml file, treat it as a single module path.
# 2. If M is a directory but does not contain the Cargo.toml file, treat its subdirectories as module paths and look for Cargo.toml files in them (the search depth is 10).
# 3. If M is not a directory, stop with an error.
ifeq ($(M),)
	MODULES := $(shell find $(DEFAULT_MODULE_PATHS) -maxdepth 10 -name "Cargo.toml" -exec dirname {} \; | sort | uniq)
else
	ifeq ($(shell [ -f $(DEFAULT_MODULE_PATHS)/Cargo.toml ] && echo yes || echo no),yes)
		MODULES := $(DEFAULT_MODULE_PATHS)
	else
		MODULES := $(shell find $(DEFAULT_MODULE_PATHS) -maxdepth 10 -name "Cargo.toml" -exec dirname {} \; | sort | uniq)
	endif
endif

.PHONY: list-modules

list-modules:
	@echo "Available modules:"
	@for module in $(MODULES); do \
		echo "  - $$module"; \
	done