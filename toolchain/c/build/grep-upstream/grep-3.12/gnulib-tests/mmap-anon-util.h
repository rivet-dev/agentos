/* Some auxiliary stuff for using mmap & friends.
   Copyright (C) 2002-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 2 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible.  */

#if defined _WIN32 && !defined __CYGWIN__

/* ------------------------ Windows ------------------------ */

# define WIN32_LEAN_AND_MEAN /* avoid including junk */
# include <windows.h>
# include <winerror.h>
# define PROT_NONE PAGE_NOACCESS
# define PROT_READ PAGE_READONLY
# define PROT_READ_WRITE PAGE_READWRITE

static void *
mmap_zeromap (void *map_addr_hint, size_t map_len)
{
  if (VirtualAlloc ((void *)((uintptr_t) map_addr_hint & -0x10000),
                    (((uintptr_t) map_addr_hint + map_len - 1) | 0xffff) + 1
                    - ((uintptr_t) map_addr_hint & -0x10000),
                    MEM_RESERVE, PAGE_NOACCESS)
      && VirtualAlloc (map_addr_hint, map_len, MEM_COMMIT, PAGE_READWRITE))
    return map_addr_hint;
  else
    return (void *)(-1);
}

int
munmap (void *addr, size_t len)
{
  if (VirtualFree (addr, len, MEM_DECOMMIT))
    return 0;
  else
    return -1;
}

int
mprotect (void *addr, size_t len, int prot)
{
  DWORD oldprot;

  if (VirtualProtect (addr, len, prot, &oldprot))
    return 0;
  else
    return -1;
}

#else

/* ------------------------ Unix ------------------------ */

# include <sys/types.h>
# include <sys/mman.h>
# include <fcntl.h>

# ifndef PROT_NONE
#  define PROT_NONE 0
# endif
# define PROT_READ_WRITE  (PROT_READ|PROT_WRITE)

# if HAVE_MAP_ANONYMOUS
#  define zero_fd -1
#  define map_flags MAP_ANONYMOUS | MAP_PRIVATE
# else
#  ifndef MAP_FILE
#   define MAP_FILE 0
#  endif
static int zero_fd;
#  define map_flags MAP_FILE | MAP_PRIVATE
# endif

static void *
mmap_zeromap (void *map_addr_hint, size_t map_len)
{
# ifdef __hpux
  /* HP-UX 10 mmap() often fails when given a hint.  So give the OS complete
     freedom about the address range.  */
  return (void *) mmap ((void *) 0,    map_len, PROT_READ_WRITE, map_flags, zero_fd, 0);
# else
  return (void *) mmap (map_addr_hint, map_len, PROT_READ_WRITE, map_flags, zero_fd, 0);
# endif
}

#endif
