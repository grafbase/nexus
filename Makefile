.PHONY: fix-format-strings
fix-format-strings:
	@echo "Fixing old-style format strings..."
	@cargo clippy --fix --allow-dirty -- -W clippy::uninlined_format_args
	@echo "✅ Format strings fixed"

.PHONY: check-format-strings
check-format-strings:
	@echo "Checking for old-style format strings..."
	@cargo clippy -- -D clippy::uninlined_format_args
	@echo "✅ All format strings use modern interpolation"