# $Copyright: $
# Copyright (c) 1996 - 2026 by Steve Baker (steve.baker.llc@gmail.com)
# All Rights reserved
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA

PREFIX=/usr/local


CC ?= gcc
INSTALL ?= install

VERSION=2.3.2
TREE_DEST=tree
DESTDIR=${PREFIX}/bin
MAN=tree.1
# Probably needs to be ${PREFIX}/share/man for most systems now
MANDIR=${PREFIX}/man
OBJS=color.o file.o filter.o hash.o html.o info.o json.o list.o tree.o unix.o \
     util.o xml.o strverscmp.o

# Uncomment options below for your particular OS:

# Linux defaults:
#LDFLAGS?=-s
CFLAGS?=-ggdb
CFLAGS?=-O3
CFLAGS+=-std=c11 -Wpedantic -Wall -Wextra -Wstrict-prototypes -Wshadow -Wconversion \
	-Wdiscarded-qualifiers
# _LARGEFILE64_SOURCE may be considered obsolete
CPPFLAGS+=-DLARGEFILE_SOURCE -D_FILE_OFFSET_BITS=64

# Uncomment for FreeBSD:
#CC=cc
#CFLAGS?=-O2
#CFLAGS+=-Wall -fomit-frame-pointer
#LDFLAGS+=-s

# Uncomment for OpenBSD:
#TREE_DEST=colortree
#MAN=colortree.1
#CFLAGS?=-O2
#CFLAGS+=-Wall -fomit-frame-pointer
#LDFLAGS+=-s

# Uncomment for Solaris:
#CC=cc
#CFLAGS+=-xO0 -v
#LDFLAGS+=
#MANDIR=${prefix}/share/man

# Uncomment for Cygwin:
#CFLAGS?=-O2
#CFLAGS+=-Wall -fomit-frame-pointer
#LDFLAGS+=-s
#TREE_DEST=tree.exe

# Uncomment for MacOS:
# It is not allowed to install to /usr/bin on MacOS any longer (SIP):
#CC = cc
#CFLAGS?=-O2
#CFLAGS+=-Wall -fomit-frame-pointer -no-cpp-precomp
#LDFLAGS+=
#MANDIR=${PREFIX}/share/man

# Uncomment for HP/UX:
#prefix=/opt
#CC=cc
# manpage of mbsrtowcs() requires C99 and the two defines
#CFLAGS+=+w -AC99
#CPPFLAGS+=-D_XOPEN_SOURCE=500 -D_POSIX_C_SOURCE=200112 
#LDFLAGS+=
#MANDIR=${PREFIX}/share/man

# Uncomment for OS/2:
#CFLAGS?=-O2
#CFLAGS+=-Wall -fomit-frame-pointer -Zomf -Zsmall-conv
#LDFLAGS+=-s -Zomf -Zsmall-conv

# Uncomment for HP NonStop:
#prefix = /opt
#CC=c89
#CFLAGS+=-Wextensions -WIEEE_float -g -Wnowarn=1506 -D_XOPEN_SOURCE_EXTENDED=1 \
#	 -Wallow_cplusplus_comments
#LDFLAGS+=

# AIX
#CC=cc_r -q64
#LD=ld -d64
#LDFLAGS+=-lc

# Android NDK
#CC=aarch64-linux-android26-clang # Need >= 26
#CFLAGS?=-O2    # Or:
#CFLAGS?=-ggdb
#CFLAGS+=-std=c89 -pedantic -Wall -Wno-error=int-conversion
#CPPFLAGS+=-D_LARGEFILE64_SOURCE -D_FILE_OFFSET_BITS=64

#------------------------------------------------------------

all:	tree

tree:	$(OBJS)
	$(CC) $(LDFLAGS) -o $(TREE_DEST) $(OBJS)

$(OBJS): %.o:	%.c tree.h
	$(CC) $(CFLAGS) $(CPPFLAGS) -c -o $@ $<

clean:
	rm -f $(TREE_DEST) *.o *~

install: tree
	$(INSTALL) -d $(DESTDIR)
	$(INSTALL) -d $(MANDIR)/man1
	$(INSTALL) $(TREE_DEST) $(DESTDIR)/$(TREE_DEST); \
	$(INSTALL) -m 644 doc/$(MAN) $(MANDIR)/man1/$(MAN)

distclean:
	rm -f *.o *~

dist:	distclean
	tar zcf ../tree-$(VERSION).tgz -C .. `cat .tarball`
