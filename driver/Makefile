all:
	make -j -C $(INSTALL_MOD_PATH)/lib/modules/$(shell uname -r)/build M=$(PWD) modules

clean:
	make -j -C $(INSTALL_MOD_PATH)/lib/modules/$(shell uname -r)/build M=$(PWD) clean
