# disc-oracle -- see README.md for the pinned Musashi commit.
CC      ?= gcc
CFLAGS  ?= -O2 -Wall -Wextra -Wno-unused-parameter
LDLIBS  := -lcrypto -lm
M       := musashi

all: disc-oracle

$(M)/m68kmake: $(M)/m68kmake.c
	$(CC) -O2 -o $@ $<

$(M)/m68kops.c $(M)/m68kops.h: $(M)/m68kmake $(M)/m68k_in.c
	cd $(M) && ./m68kmake . m68k_in.c

disc-oracle: disc-oracle.c $(M)/m68kcpu.c $(M)/m68kops.c $(M)/m68kdasm.c $(M)/softfloat/softfloat.c
	$(CC) $(CFLAGS) -I$(M) -o $@ $^ $(LDLIBS)

clean:
	rm -f disc-oracle $(M)/m68kmake $(M)/m68kops.c $(M)/m68kops.h
.PHONY: all clean
