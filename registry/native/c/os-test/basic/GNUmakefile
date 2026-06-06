include ../misc/GNUmakefile.suite

ifneq ($(NO_SHARED),1)
.SUFFIXES: .c .so

all: dlfcn/dlclose.so dlfcn/dlopen.so dlfcn/dlsym.so

.c.so:
	$(CC) $(SHARED_CFLAGS) $(CFLAGS) $(CPPFLAGS) -DSHARED $< -o $@ $(LDFLAGS)

$(OUT_PATH)/dlfcn/dlclose.out: dlfcn/dlclose dlfcn/dlclose.so
	../misc/run.sh dlfcn/dlclose $(OUT_PATH)/dlfcn/dlclose.out
$(OUT_PATH)/dlfcn/dlopen.out: dlfcn/dlopen dlfcn/dlopen.so
	../misc/run.sh dlfcn/dlopen $(OUT_PATH)/dlfcn/dlopen.out
$(OUT_PATH)/dlfcn/dlsym.out: dlfcn/dlsym dlfcn/dlsym.so
	../misc/run.sh dlfcn/dlsym $(OUT_PATH)/dlfcn/dlsym.out
endif

clean: clean-so

.PHONY: clean-so
clean-so:
	rm -f dlfcn/*.so
