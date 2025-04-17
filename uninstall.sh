#!/bin/bash
# Exit on error
set -e
echo "Uninstalling Clipboard Logger Service..."
# Stop the service if it's running
systemctl --user stop clipboard-logger.service

# Disable the service
systemctl --user disable clipboard-logger.service

# Remove the service file
echo "Removing systemd service..."
rm -f ~/.config/systemd/user/clipboard-logger.service

# Remove the binary
echo "Removing binary..."
rm -f ~/.local/bin/clipboard-logger
