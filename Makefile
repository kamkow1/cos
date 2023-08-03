MODE = debug
BIN = target/x86_64-cos/$(MODE)/bootimage-cos.bin
IMG = disk.img
QEMUFLAGS = -drive file=$(IMG),format=raw \
			-serial stdio

.PHONY: build
build:
	qemu-img create $(IMG) 32M
	cargo bootimage
	dd conv=notrunc if=$(BIN) of=$(IMG)

.PHONY: run
run:
	qemu-system-x86_64 $(QEMUFLAGS)
