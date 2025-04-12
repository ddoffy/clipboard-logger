#!/bin/bash

# Exit on error
set -e 

echo  "Instlling Clipboard Logger Service..."

# Compile the application
echo "Building application..."
cargo build --release

# Create necessary directories
echo "Creating directories..."
mkdir -p ~/.local/bin
mkdir -p ~/.config/systemd/user

# Copy the binary to user's local bin
echo "Installing binary..."
cp target/release/clipboard-logger ~/.local/bin/

# Copy systemd service file
echo "Installing systemd service..."
cp  clipboard-logger.service ~/.config/systemd/user/

# Reload systemd daemon
systemctl --user daemon-reload

echo "Installation complete!"
echo ""
echo "To enable and start the service, run:"
echo "systemctl --user enable clipboard-logger.service"
echo "systemctl --user start clipboard-logger.service"
echo ""
echo "To check state:"
echo "systemctl --user status clipboard-logger.service"
echo ""
echo "To view logs:"
echo "journalctl --user -u clipboard-logger.service"
echo ""
echo "Manual usage:"
echo "clipboard_logger --help"
