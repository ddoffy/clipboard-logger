!/bin/bash
# Exit on error
set -e
echo "Uninstalling Clipboard Logger Service..."
# Stop the service if it's running
systemctl --user stop clipboard-logger.service

# Disable the service
systemctl --user disable clipboard-logger.service
