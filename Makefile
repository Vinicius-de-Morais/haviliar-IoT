# ======================================
# Makefile para projeto ESP32 + Rust
# ======================================

# Nome do alvo
TARGET = xtensa-esp32-espidf

# Porta serial padrão (ajuste conforme necessário)
# Linux: /dev/ttyUSB0 ou /dev/ttySx
# Windows: COM7, COM8...
PORT ?= /dev/ttyUSB0

# Detecta SO (Linux ou Windows)
OS := $(shell uname -s)

# Binários
CARGO = cargo
ESPFLASH = espflash
ESP_ENV = source ~/export-esp.sh

# ========================
# Comandos principais
# ========================

# Build
build:
	@echo "==> Compilando para ESP32"
	$(ESP_ENV) && $(CARGO) build --release --target $(TARGET)

# Build + Run (simula no host ou executa diretamente no ESP32, dependendo do target)
run:
	@echo "==> Build + Run"
	$(ESP_ENV) && $(CARGO) run --target $(TARGET)

# Flash
flash:
	@echo "==> Gravando firmware na placa"
	$(ESP_ENV) && $(ESPFLASH) flash --monitor --baud 115200 $(PORT) target/$(TARGET)/release/$(shell basename $(CURDIR))

# Monitor serial
monitor:
	@echo "==> Monitor serial"
	$(ESPFLASH) monitor $(PORT)

# Limpar build
clean:
	@echo "==> Limpando build"
	$(CARGO) clean

# ========================
# Setup do ambiente
# ========================

setup-linux:
	@echo "==> Instalando ferramentas necessárias no Linux"
	sudo pacman -S cmake ninja gcc git pkg-config llvm clang
	cargo install espup --locked
	espup install
	. /home/winux/export-esp.sh
	@echo "==> Criando link simbólico para corrigir libxml2 (Arch Linux)"
	sudo ln -sf /usr/lib/libxml2.so.16 /usr/lib/libxml2.so.2
	@echo "==> Adicionando permissão de usuário para acessar porta serial"
	sudo usermod -a -G uucp $(USER)
	sudo usermod -a -G dialout $(USER)
	@echo "==> Setup finalizado! Reinicie a sessão para aplicar mudanças."

setup-windows:
	@echo "==> No Windows, instale manualmente:"
	@echo "   - Rust (via rustup ou mise)"
	@echo "   - espup: cargo install espup --locked"
	@echo "   - cargo-generate: cargo install cargo-generate"
	@echo "   - Espressif drivers: https://www.silabs.com/developers/usb-to-uart-bridge-vcp-drivers"

# ========================
# Ajuda
# ========================

help:
	@echo "Comandos disponíveis:"
	@echo "  make build         - Compila para ESP32"
	@echo "  make run           - Build + Run"
	@echo "  make flash         - Grava firmware no ESP32"
	@echo "  make monitor       - Monitor serial"
	@echo "  make clean         - Limpa build"
	@echo "  make setup-linux   - Configura ambiente Linux"
	@echo "  make setup-windows - Instruções para Windows"
