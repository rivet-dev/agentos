/* Determine the virtual memory area of a given address.
   Copyright (C) 2002-2025 Free Software Foundation, Inc.
   Copyright (C) 2003-2006  Paolo Bonzini <bonzini@gnu.org>

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

/* Written by Bruno Haible and Paolo Bonzini.  */

#include <config.h>

/* On Solaris in 32-bit mode, when gnulib module 'largefile' is in use,
   prevent a compilation error
     "Cannot use procfs in the large file compilation environment"
   while also preventing <sys/types.h> from not defining off_t.
   On Android, when targeting Android 4.4 or older with a GCC toolchain,
   prevent a compilation error
     "error: call to 'mmap' declared with attribute error: mmap is not
      available with _FILE_OFFSET_BITS=64 when using GCC until android-21.
      Either raise your minSdkVersion, disable _FILE_OFFSET_BITS=64, or
      switch to Clang."
   The files that we access in this compilation unit are less than 2 GB
   large.  */
#if defined __sun && !defined _LP64 && _FILE_OFFSET_BITS == 64
# undef _FILE_OFFSET_BITS
# define _FILE_OFFSET_BITS 32
#endif
#ifdef __ANDROID__
# undef _FILE_OFFSET_BITS
#endif

/* Specification.  */
#include "stackvma.h"

#include <stdio.h>
#include <stdlib.h>

/* =========================== stackvma-simple.c =========================== */

#if defined __linux__ || defined __ANDROID__ \
    || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
    || defined __NetBSD__ \
    || (defined __APPLE__ && defined __MACH__) \
    || defined _AIX || defined __sgi || defined __sun \
    || defined __CYGWIN__ || defined __HAIKU__

/* This file contains the proximity test function for the simple cases, where
   the OS has an API for enumerating the mapped ranges of virtual memory.  */

# if STACK_DIRECTION < 0

/* Info about the gap between this VMA and the previous one.
   addr must be < vma->start.  */
static int
simple_is_near_this (uintptr_t addr, struct vma_struct *vma)
{
  return (vma->start - addr <= (vma->start - vma->prev_end) / 2);
}

# endif
# if STACK_DIRECTION > 0

/* Info about the gap between this VMA and the next one.
   addr must be > vma->end - 1.  */
static int
simple_is_near_this (uintptr_t addr, struct vma_struct *vma)
{
  return (addr - vma->end < (vma->next_start - vma->end) / 2);
}

# endif

#endif

/* =========================== stackvma-rofile.c =========================== */
/* Buffered read-only streams.  */

#if defined __linux__ || defined __ANDROID__ \
    || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
    || defined __NetBSD__ \
    || defined __CYGWIN__

# include <errno.h> /* errno, EINTR */
# include <fcntl.h> /* open, O_RDONLY */
# include <stddef.h> /* size_t */
# include <unistd.h> /* getpagesize, lseek, read, close */
# include <sys/types.h>
# include <sys/mman.h> /* mmap, munmap */

# if defined __linux__ || defined __ANDROID__
#  include <limits.h> /* PATH_MAX */
# endif

/* Buffered read-only streams.
   We cannot use <stdio.h> here, because fopen() calls malloc(), and a malloc()
   call may have been interrupted.
   Also, we cannot use multiple read() calls, because if the buffer size is
   smaller than the file's contents:
     - On NetBSD, the second read() call would return 0, thus making the file
       appear truncated.
     - On DragonFly BSD, the first read() call would fail with errno = EFBIG.
     - On all platforms, if some other thread is doing memory allocations or
       deallocations between two read() calls, there is a high risk that the
       result of these two read() calls don't fit together, and as a
       consequence we will parse garbage and either omit some VMAs or return
       VMAs with nonsensical addresses.
   So use mmap(), and ignore the resulting VMA.
   The stack-allocated buffer cannot be too large, because this can be called
   when we are in the context of an alternate stack of just SIGSTKSZ bytes.  */

# if defined __linux__ || defined __ANDROID__
  /* On Linux, if the file does not entirely fit into the buffer, the read()
     function stops before the line that would come out truncated.  The
     maximum size of such a line is 73 + PATH_MAX bytes.  To be sure that we
     have read everything, we must verify that at least that many bytes are
     left when read() returned.  */
#  define MIN_LEFTOVER (73 + PATH_MAX)
# else
#  define MIN_LEFTOVER 1
# endif

# if MIN_LEFTOVER < 1024
#  define STACK_ALLOCATED_BUFFER_SIZE 1024
# else
  /* There is no point in using a stack-allocated buffer if it is too small
     anyway.  */
#  define STACK_ALLOCATED_BUFFER_SIZE 1
# endif

struct rofile
  {
    size_t position;
    size_t filled;
    int eof_seen;
    /* These fields deal with allocation of the buffer.  */
    char *buffer;
    char *auxmap;
    size_t auxmap_length;
    uintptr_t auxmap_start;
    uintptr_t auxmap_end;
    char stack_allocated_buffer[STACK_ALLOCATED_BUFFER_SIZE];
  };

/* Open a read-only file stream.  */
static int
rof_open (struct rofile *rof, const char *filename)
{
  int fd;
  uintptr_t pagesize;
  size_t size;

  fd = open (filename, O_RDONLY);
  if (fd < 0)
    return -1;
  rof->position = 0;
  rof->eof_seen = 0;
  /* Try the static buffer first.  */
  pagesize = 0;
  rof->buffer = rof->stack_allocated_buffer;
  size = sizeof (rof->stack_allocated_buffer);
  rof->auxmap = NULL;
  rof->auxmap_start = 0;
  rof->auxmap_end = 0;
  for (;;)
    {
      /* Attempt to read the contents in a single system call.  */
      if (size > MIN_LEFTOVER)
        {
          ssize_t n = read (fd, rof->buffer, size);
          if (n < 0 && errno == EINTR)
            goto retry;
# if defined __DragonFly__
          if (!(n < 0 && errno == EFBIG))
# endif
            {
              if (n <= 0)
                /* Empty file.  */
                goto fail1;
              if (MIN_LEFTOVER <= size - n)
                {
                  /* The buffer was sufficiently large.  */
                  rof->filled = n;
# if defined __linux__ || defined __ANDROID__
                  /* On Linux, the read() call may stop even if the buffer was
                     large enough.  We need the equivalent of full_read().  */
                  for (;;)
                    {
                      n = read (fd, rof->buffer + rof->filled, size - rof->filled);
                      if (n < 0 && errno == EINTR)
                        goto retry;
                      if (n < 0)
                        /* Some error.  */
                        goto fail1;
                      if (n == 0)
                        {
                          /* Reached the end of file.  */
                          close (fd);
                          return 0;
                        }
                      if (size - rof->filled - n < MIN_LEFTOVER)
                        /* Allocate a larger buffer.  */
                        break;
                      rof->filled += n;
                    }
# else
                  close (fd);
                  return 0;
# endif
                }
            }
        }
      /* Allocate a larger buffer.  */
      if (pagesize == 0)
        {
          pagesize = getpagesize ();
          size = pagesize;
          while (size <= MIN_LEFTOVER)
            size = 2 * size;
        }
      else
        {
          size = 2 * size;
          if (size == 0)
            /* Wraparound.  */
            goto fail1;
          if (rof->auxmap != NULL)
            munmap (rof->auxmap, rof->auxmap_length);
        }
      rof->auxmap = (void *) mmap ((void *) 0, size, PROT_READ | PROT_WRITE,
                                   MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
      if (rof->auxmap == (void *) -1)
        {
          close (fd);
          return -1;
        }
      rof->auxmap_length = size;
      rof->auxmap_start = (uintptr_t) rof->auxmap;
      rof->auxmap_end = rof->auxmap_start + size;
      rof->buffer = (char *) rof->auxmap;
     retry:
      /* Restart.  */
      if (lseek (fd, 0, SEEK_SET) < 0)
        {
          close (fd);
          fd = open (filename, O_RDONLY);
          if (fd < 0)
            goto fail2;
        }
    }
 fail1:
  close (fd);
 fail2:
  if (rof->auxmap != NULL)
    munmap (rof->auxmap, rof->auxmap_length);
  return -1;
}

/* Return the next byte from a read-only file stream without consuming it,
   or -1 at EOF.  */
static int
rof_peekchar (struct rofile *rof)
{
  if (rof->position == rof->filled)
    {
      rof->eof_seen = 1;
      return -1;
    }
  return (unsigned char) rof->buffer[rof->position];
}

/* Return the next byte from a read-only file stream, or -1 at EOF.  */
static int
rof_getchar (struct rofile *rof)
{
  int c = rof_peekchar (rof);
  if (c >= 0)
    rof->position++;
  return c;
}

/* Parse an unsigned hexadecimal number from a read-only file stream.  */
static int
rof_scanf_lx (struct rofile *rof, uintptr_t *valuep)
{
  uintptr_t value = 0;
  unsigned int numdigits = 0;
  for (;;)
    {
      int c = rof_peekchar (rof);
      if (c >= '0' && c <= '9')
        value = (value << 4) + (c - '0');
      else if (c >= 'A' && c <= 'F')
        value = (value << 4) + (c - 'A' + 10);
      else if (c >= 'a' && c <= 'f')
        value = (value << 4) + (c - 'a' + 10);
      else
        break;
      rof_getchar (rof);
      numdigits++;
    }
  if (numdigits == 0)
    return -1;
  *valuep = value;
  return 0;
}

/* Close a read-only file stream.  */
static void
rof_close (struct rofile *rof)
{
  if (rof->auxmap != NULL)
    munmap (rof->auxmap, rof->auxmap_length);
}

#endif

/* ========================== stackvma-vma-iter.c ========================== */
/* Iterate through the virtual memory areas of the current process,
   by reading from the /proc file system.  */

/* This code is a simplified copy (no handling of protection flags) of the
   code in gnulib's lib/vma-iter.c.  */

#if defined __linux__ || defined __ANDROID__ \
    || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
    || defined __NetBSD__ \
    || defined __CYGWIN__

/* Forward declarations.  */
struct callback_locals;
static int callback (struct callback_locals *locals, uintptr_t start, uintptr_t end);

# if defined __linux__ || defined __ANDROID__ || (defined __FreeBSD_kernel__ && !defined __FreeBSD__) || defined __CYGWIN__
/* GNU/kFreeBSD mounts /proc as linprocfs, which looks like a Linux /proc
   file system.  */

static int
vma_iterate_proc (struct callback_locals *locals)
{
  struct rofile rof;

  /* Open the current process' maps file.  It describes one VMA per line.  */
  if (rof_open (&rof, "/proc/self/maps") >= 0)
    {
      uintptr_t auxmap_start = rof.auxmap_start;
      uintptr_t auxmap_end = rof.auxmap_end;

      for (;;)
        {
          uintptr_t start, end;
          int c;

          /* Parse one line.  First start and end.  */
          if (!(rof_scanf_lx (&rof, &start) >= 0
                && rof_getchar (&rof) == '-'
                && rof_scanf_lx (&rof, &end) >= 0))
            break;
          while (c = rof_getchar (&rof), c != -1 && c != '\n')
            ;

          if (start <= auxmap_start && auxmap_end - 1 <= end - 1)
            {
              /* Consider [start,end-1] \ [auxmap_start,auxmap_end-1]
                 = [start,auxmap_start-1] u [auxmap_end,end-1].  */
              if (start < auxmap_start)
                if (callback (locals, start, auxmap_start))
                  break;
              if (auxmap_end - 1 < end - 1)
                if (callback (locals, auxmap_end, end))
                  break;
            }
          else
            {
              if (callback (locals, start, end))
                break;
            }
        }
      rof_close (&rof);
      return 0;
    }

  return -1;
}

# elif defined __FreeBSD__ || defined __DragonFly__ || defined __NetBSD__

static int
vma_iterate_proc (struct callback_locals *locals)
{
  struct rofile rof;

  /* Open the current process' maps file.  It describes one VMA per line.
     On FreeBSD:
       Cf. <https://www.freebsd.org/cgi/cvsweb.cgi/src/sys/fs/procfs/procfs_map.c?annotate=HEAD>
     On NetBSD, there are two such files:
       - /proc/curproc/map in near-FreeBSD syntax,
       - /proc/curproc/maps in Linux syntax.
       Cf. <https://cvsweb.netbsd.org/bsdweb.cgi/src/sys/miscfs/procfs/procfs_map.c?rev=HEAD> */
  if (rof_open (&rof, "/proc/curproc/map") >= 0)
    {
      uintptr_t auxmap_start = rof.auxmap_start;
      uintptr_t auxmap_end = rof.auxmap_end;

      for (;;)
        {
          uintptr_t start, end;
          int c;

          /* Parse one line.  First start.  */
          if (!(rof_getchar (&rof) == '0'
                && rof_getchar (&rof) == 'x'
                && rof_scanf_lx (&rof, &start) >= 0))
            break;
          while (c = rof_peekchar (&rof), c == ' ' || c == '\t')
            rof_getchar (&rof);
          /* Then end.  */
          if (!(rof_getchar (&rof) == '0'
                && rof_getchar (&rof) == 'x'
                && rof_scanf_lx (&rof, &end) >= 0))
            break;
          while (c = rof_getchar (&rof), c != -1 && c != '\n')
            ;

          if (start <= auxmap_start && auxmap_end - 1 <= end - 1)
            {
              /* Consider [start,end-1] \ [auxmap_start,auxmap_end-1]
                 = [start,auxmap_start-1] u [auxmap_end,end-1].  */
              if (start < auxmap_start)
                if (callback (locals, start, auxmap_start))
                  break;
              if (auxmap_end - 1 < end - 1)
                if (callback (locals, auxmap_end, end))
                  break;
            }
          else
            {
              if (callback (locals, start, end))
                break;
            }
        }
      rof_close (&rof);
      return 0;
    }

  return -1;
}

# endif

# if (defined __FreeBSD_kernel__ || defined __FreeBSD__) && defined KERN_PROC_VMMAP /* FreeBSD >= 7.1 */

#  include <sys/user.h> /* struct kinfo_vmentry */
#  include <sys/sysctl.h> /* sysctl */

static int
vma_iterate_bsd (struct callback_locals *locals)
{
  /* Documentation: https://www.freebsd.org/cgi/man.cgi?sysctl(3)  */
  int info_path[] = { CTL_KERN, KERN_PROC, KERN_PROC_VMMAP, getpid () };
  size_t len;
  size_t pagesize;
  size_t memneed;
  void *auxmap;
  unsigned long auxmap_start;
  unsigned long auxmap_end;
  char *mem;
  char *p;
  char *p_end;

  len = 0;
  if (sysctl (info_path, 4, NULL, &len, NULL, 0) < 0)
    return -1;
  /* Allow for small variations over time.  In a multithreaded program
     new VMAs can be allocated at any moment.  */
  len = 2 * len + 200;
  /* Allocate memneed bytes of memory.
     We cannot use alloca here, because not much stack space is guaranteed.
     We also cannot use malloc here, because a malloc() call may call mmap()
     and thus pre-allocate available memory.
     So use mmap(), and ignore the resulting VMA.  */
  pagesize = getpagesize ();
  memneed = len;
  memneed = ((memneed - 1) / pagesize + 1) * pagesize;
  auxmap = (void *) mmap ((void *) 0, memneed, PROT_READ | PROT_WRITE,
                          MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
  if (auxmap == (void *) -1)
    return -1;
  auxmap_start = (unsigned long) auxmap;
  auxmap_end = auxmap_start + memneed;
  mem = (char *) auxmap;
  if (sysctl (info_path, 4, mem, &len, NULL, 0) < 0)
    {
      munmap (auxmap, memneed);
      return -1;
    }
  p = mem;
  p_end = mem + len;
  while (p < p_end)
    {
      struct kinfo_vmentry *kve = (struct kinfo_vmentry *) p;
      unsigned long start = kve->kve_start;
      unsigned long end = kve->kve_end;
      if (start <= auxmap_start && auxmap_end - 1 <= end - 1)
        {
          /* Consider [start,end-1] \ [auxmap_start,auxmap_end-1]
             = [start,auxmap_start-1] u [auxmap_end,end-1].  */
          if (start < auxmap_start)
            if (callback (locals, start, auxmap_start))
              break;
          if (auxmap_end - 1 < end - 1)
            if (callback (locals, auxmap_end, end))
              break;
        }
      else
        {
          if (callback (locals, start, end))
            break;
        }
      p += kve->kve_structsize;
    }
  munmap (auxmap, memneed);
  return 0;
}

# else

#  define vma_iterate_bsd(locals) (-1)

# endif


/* Iterate over the virtual memory areas of the current process.
   If such iteration is supported, the callback is called once for every
   virtual memory area, in ascending order, with the following arguments:
     - LOCALS is the same argument as passed to vma_iterate.
     - START is the address of the first byte in the area, page-aligned.
     - END is the address of the last byte in the area plus 1, page-aligned.
       Note that it may be 0 for the last area in the address space.
   If the callback returns 0, the iteration continues.  If it returns 1,
   the iteration terminates prematurely.
   This function may open file descriptors, but does not call malloc().
   Return 0 if all went well, or -1 in case of error.  */
static int
vma_iterate (struct callback_locals *locals)
{
# if defined __FreeBSD__
  /* On FreeBSD with procfs (but not GNU/kFreeBSD, which uses linprocfs), the
     function vma_iterate_proc does not return the virtual memory areas that
     were created by anonymous mmap.  See
     <https://svnweb.freebsd.org/base/head/sys/fs/procfs/procfs_map.c?view=markup>
     So use vma_iterate_proc only as a fallback.  */
  int retval = vma_iterate_bsd (locals);
  if (retval == 0)
      return 0;

  return vma_iterate_proc (locals);
# else
  /* On the other platforms, try the /proc approach first, and the sysctl()
     as a fallback.  */
  int retval = vma_iterate_proc (locals);
  if (retval == 0)
      return 0;

  return vma_iterate_bsd (locals);
# endif
}

#endif

/* =========================== stackvma-mincore.c =========================== */

/* mincore() is a system call that allows to inquire the status of a
   range of pages of virtual memory.  In particular, it allows to inquire
   whether a page is mapped at all (except on Mac OS X, where mincore
   returns 0 even for unmapped addresses).
   As of 2006, mincore() is supported by:        possible bits:
     - Linux,   since Linux 2.4 and glibc 2.2,   1
     - Solaris, since Solaris 9,                 1
     - MacOS X, since MacOS X 10.3 (at least),   1
     - FreeBSD, since FreeBSD 6.0,               MINCORE_{INCORE,REFERENCED,MODIFIED}
     - NetBSD,  since NetBSD 3.0 (at least),     1
     - OpenBSD, since OpenBSD 2.6 (at least),    1
     - AIX,     since AIX 5.3,                   1
   As of 2019, also on
     - Hurd.
   However, while the API allows to easily determine the bounds of mapped
   virtual memory, it does not make it easy to find the bounds of _unmapped_
   virtual memory ranges.  We try to work around this, but it may still be
   slow.  */

#if defined __linux__ || defined __ANDROID__ \
    || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
    || defined __NetBSD__ /* || defined __OpenBSD__ */ \
    /* || (defined __APPLE__ && defined __MACH__) */ \
    || defined _AIX || defined __sun

# include <unistd.h> /* getpagesize, mincore */
# include <sys/types.h>
# include <sys/mman.h> /* mincore */

/* The AIX declaration of mincore() uses 'caddr_t', whereas the other platforms
   use 'void *'. */
# ifdef _AIX
typedef caddr_t MINCORE_ADDR_T;
# else
typedef void* MINCORE_ADDR_T;
# endif

/* The glibc and musl declaration of mincore() uses 'unsigned char *', whereas
   the BSD declaration uses 'char *'.  */
# if __GLIBC__ >= 2 || defined __linux__ || defined __ANDROID__
typedef unsigned char pageinfo_t;
# else
typedef char pageinfo_t;
# endif

/* Cache for getpagesize().  */
static uintptr_t pagesize;

/* Initialize pagesize.  */
static void
init_pagesize (void)
{
  pagesize = getpagesize ();
}

/* Test whether the page starting at ADDR is among the address range.
   ADDR must be a multiple of pagesize.  */
static int
is_mapped (uintptr_t addr)
{
  pageinfo_t vec[1];
  return mincore ((MINCORE_ADDR_T) addr, pagesize, vec) >= 0;
}

/* Assuming that the page starting at ADDR is among the address range,
   return the start of its virtual memory range.
   ADDR must be a multiple of pagesize.  */
static uintptr_t
mapped_range_start (uintptr_t addr)
{
  /* Use a moderately sized VEC here, small enough that it fits on the stack
     (without requiring malloc).  */
  pageinfo_t vec[1024];
  uintptr_t stepsize = sizeof (vec);

  for (;;)
    {
      uintptr_t max_remaining;

      if (addr == 0)
        return addr;

      max_remaining = addr / pagesize;
      if (stepsize > max_remaining)
        stepsize = max_remaining;
      if (mincore ((MINCORE_ADDR_T) (addr - stepsize * pagesize),
                   stepsize * pagesize, vec) < 0)
        /* Time to search in smaller steps.  */
        break;
      /* The entire range exists.  Continue searching in large steps.  */
      addr -= stepsize * pagesize;
    }
  for (;;)
    {
      uintptr_t halfstepsize1;
      uintptr_t halfstepsize2;

      if (stepsize == 1)
        return addr;

      /* Here we know that less than stepsize pages exist starting at addr.  */
      halfstepsize1 = (stepsize + 1) / 2;
      halfstepsize2 = stepsize / 2;
      /* halfstepsize1 + halfstepsize2 = stepsize.  */

      if (mincore ((MINCORE_ADDR_T) (addr - halfstepsize1 * pagesize),
                   halfstepsize1 * pagesize, vec) < 0)
        stepsize = halfstepsize1;
      else
        {
          addr -= halfstepsize1 * pagesize;
          stepsize = halfstepsize2;
        }
    }
}

/* Assuming that the page starting at ADDR is among the address range,
   return the end of its virtual memory range + 1.
   ADDR must be a multiple of pagesize.  */
static uintptr_t
mapped_range_end (uintptr_t addr)
{
  /* Use a moderately sized VEC here, small enough that it fits on the stack
     (without requiring malloc).  */
  pageinfo_t vec[1024];
  uintptr_t stepsize = sizeof (vec);

  addr += pagesize;
  for (;;)
    {
      uintptr_t max_remaining;

      if (addr == 0) /* wrapped around? */
        return addr;

      max_remaining = (- addr) / pagesize;
      if (stepsize > max_remaining)
        stepsize = max_remaining;
      if (mincore ((MINCORE_ADDR_T) addr, stepsize * pagesize, vec) < 0)
        /* Time to search in smaller steps.  */
        break;
      /* The entire range exists.  Continue searching in large steps.  */
      addr += stepsize * pagesize;
    }
  for (;;)
    {
      uintptr_t halfstepsize1;
      uintptr_t halfstepsize2;

      if (stepsize == 1)
        return addr;

      /* Here we know that less than stepsize pages exist starting at addr.  */
      halfstepsize1 = (stepsize + 1) / 2;
      halfstepsize2 = stepsize / 2;
      /* halfstepsize1 + halfstepsize2 = stepsize.  */

      if (mincore ((MINCORE_ADDR_T) addr, halfstepsize1 * pagesize, vec) < 0)
        stepsize = halfstepsize1;
      else
        {
          addr += halfstepsize1 * pagesize;
          stepsize = halfstepsize2;
        }
    }
}

/* Determine whether an address range [ADDR1..ADDR2] is completely unmapped.
   ADDR1 must be <= ADDR2.  */
static int
is_unmapped (uintptr_t addr1, uintptr_t addr2)
{
  uintptr_t count;
  uintptr_t stepsize;

  /* Round addr1 down.  */
  addr1 = (addr1 / pagesize) * pagesize;
  /* Round addr2 up and turn it into an exclusive bound.  */
  addr2 = ((addr2 / pagesize) + 1) * pagesize;

  /* This is slow: mincore() does not provide a way to determine the bounds
     of the gaps directly.  So we have to use mincore() on individual pages
     over and over again.  Only after we've verified that all pages are
     unmapped, we know that the range is completely unmapped.
     If we were to traverse the pages from bottom to top or from top to bottom,
     it would be slow even in the average case.  To speed up the search, we
     exploit the fact that mapped memory ranges are larger than one page on
     average, therefore we have good chances of hitting a mapped area if we
     traverse only every second, or only fourth page, etc.  This doesn't
     decrease the worst-case runtime, only the average runtime.  */
  count = (addr2 - addr1) / pagesize;
  /* We have to test is_mapped (addr1 + i * pagesize) for 0 <= i < count.  */
  for (stepsize = 1; stepsize < count; )
    stepsize = 2 * stepsize;
  for (;;)
    {
      uintptr_t addr_stepsize;
      uintptr_t i;
      uintptr_t addr;

      stepsize = stepsize / 2;
      if (stepsize == 0)
        break;
      addr_stepsize = stepsize * pagesize;
      for (i = stepsize, addr = addr1 + addr_stepsize;
           i < count;
           i += 2 * stepsize, addr += 2 * addr_stepsize)
        /* Here addr = addr1 + i * pagesize.  */
        if (is_mapped (addr))
          return 0;
    }
  return 1;
}

# if STACK_DIRECTION < 0

/* Info about the gap between this VMA and the previous one.
   addr must be < vma->start.  */
static int
mincore_is_near_this (uintptr_t addr, struct vma_struct *vma)
{
  /*   vma->start - addr <= (vma->start - vma->prev_end) / 2
     is mathematically equivalent to
       vma->prev_end <= 2 * addr - vma->start
     <==> is_unmapped (2 * addr - vma->start, vma->start - 1).
     But be careful about overflow: if 2 * addr - vma->start is negative,
     we consider a tiny "guard page" mapping [0, 0] to be present around
     NULL; it intersects the range (2 * addr - vma->start, vma->start - 1),
     therefore return false.  */
  uintptr_t testaddr = addr - (vma->start - addr);
  if (testaddr > addr) /* overflow? */
    return 0;
  /* Here testaddr <= addr < vma->start.  */
  return is_unmapped (testaddr, vma->start - 1);
}

# endif
# if STACK_DIRECTION > 0

/* Info about the gap between this VMA and the next one.
   addr must be > vma->end - 1.  */
static int
mincore_is_near_this (uintptr_t addr, struct vma_struct *vma)
{
  /*   addr - vma->end < (vma->next_start - vma->end) / 2
     is mathematically equivalent to
       vma->next_start > 2 * addr - vma->end
     <==> is_unmapped (vma->end, 2 * addr - vma->end).
     But be careful about overflow: if 2 * addr - vma->end is > ~0UL,
     we consider a tiny "guard page" mapping [0, 0] to be present around
     NULL; it intersects the range (vma->end, 2 * addr - vma->end),
     therefore return false.  */
  uintptr_t testaddr = addr + (addr - vma->end);
  if (testaddr < addr) /* overflow? */
    return 0;
  /* Here vma->end - 1 < addr <= testaddr.  */
  return is_unmapped (vma->end, testaddr);
}

# endif

static int
mincore_get_vma (uintptr_t address, struct vma_struct *vma)
{
  if (pagesize == 0)
    init_pagesize ();
  address = (address / pagesize) * pagesize;
  vma->start = mapped_range_start (address);
  vma->end = mapped_range_end (address);
  vma->is_near_this = mincore_is_near_this;
  return 0;
}

#endif

/* ========================================================================== */

/* ---------------------------- stackvma-linux.c ---------------------------- */

#if defined __linux__ || defined __ANDROID__ /* Linux */

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
# if STACK_DIRECTION < 0
  uintptr_t prev;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
# if STACK_DIRECTION < 0
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->vma->prev_end = locals->prev;
      locals->retval = 0;
      return 1;
    }
  locals->prev = end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  return 0;
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct callback_locals locals;
  locals.address = address;
  locals.vma = vma;
# if STACK_DIRECTION < 0
  locals.prev = 0;
# else
  locals.stop_at_next_vma = 0;
# endif
  locals.retval = -1;

  vma_iterate (&locals);
  if (locals.retval == 0)
    {
# if !(STACK_DIRECTION < 0)
      if (locals.stop_at_next_vma)
        vma->next_start = 0;
# endif
      vma->is_near_this = simple_is_near_this;
      return 0;
    }

  return mincore_get_vma (address, vma);
}

/* --------------------------- stackvma-freebsd.c --------------------------- */

#elif defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ /* GNU/kFreeBSD, FreeBSD */

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
  /* The stack appears as multiple adjacent segments, therefore we
     merge adjacent segments.  */
  uintptr_t curr_start, curr_end;
# if STACK_DIRECTION < 0
  uintptr_t prev_end;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
  if (start == locals->curr_end)
    {
      /* Merge adjacent segments.  */
      locals->curr_end = end;
      return 0;
    }
# if STACK_DIRECTION < 0
  if (locals->curr_start < locals->curr_end
      && locals->address >= locals->curr_start
      && locals->address <= locals->curr_end - 1)
    {
      locals->vma->start = locals->curr_start;
      locals->vma->end = locals->curr_end;
      locals->vma->prev_end = locals->prev_end;
      locals->retval = 0;
      return 1;
    }
  locals->prev_end = locals->curr_end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = locals->curr_start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->curr_start < locals->curr_end
      && locals->address >= locals->curr_start
      && locals->address <= locals->curr_end - 1)
    {
      locals->vma->start = locals->curr_start;
      locals->vma->end = locals->curr_end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  locals->curr_start = start; locals->curr_end = end;
  return 0;
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct callback_locals locals;
  locals.address = address;
  locals.vma = vma;
  locals.curr_start = 0;
  locals.curr_end = 0;
# if STACK_DIRECTION < 0
  locals.prev_end = 0;
# else
  locals.stop_at_next_vma = 0;
# endif
  locals.retval = -1;

  vma_iterate (&locals);
  if (locals.retval < 0)
    {
      if (locals.curr_start < locals.curr_end
          && address >= locals.curr_start && address <= locals.curr_end - 1)
        {
          vma->start = locals.curr_start;
          vma->end = locals.curr_end;
# if STACK_DIRECTION < 0
          vma->prev_end = locals.prev_end;
# else
          vma->next_start = 0;
# endif
          locals.retval = 0;
        }
    }
  if (locals.retval == 0)
    {
# if !(STACK_DIRECTION < 0)
      if (locals.stop_at_next_vma)
        vma->next_start = 0;
# endif
      vma->is_near_this = simple_is_near_this;
      return 0;
    }

  /* FreeBSD 6.[01] doesn't allow to distinguish unmapped pages from
     mapped but swapped-out pages.  See whether it's fixed.  */
  if (!is_mapped (0))
    /* OK, mincore() appears to work as expected.  */
    return mincore_get_vma (address, vma);
  return -1;
}

/* --------------------------- stackvma-netbsd.c --------------------------- */

#elif defined __NetBSD__ /* NetBSD */

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
  /* The stack appears as multiple adjacent segments, therefore we
     merge adjacent segments.  */
  uintptr_t curr_start, curr_end;
# if STACK_DIRECTION < 0
  uintptr_t prev_end;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
  if (start == locals->curr_end)
    {
      /* Merge adjacent segments.  */
      locals->curr_end = end;
      return 0;
    }
# if STACK_DIRECTION < 0
  if (locals->curr_start < locals->curr_end
      && locals->address >= locals->curr_start
      && locals->address <= locals->curr_end - 1)
    {
      locals->vma->start = locals->curr_start;
      locals->vma->end = locals->curr_end;
      locals->vma->prev_end = locals->prev_end;
      locals->retval = 0;
      return 1;
    }
  locals->prev_end = locals->curr_end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = locals->curr_start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->curr_start < locals->curr_end
      && locals->address >= locals->curr_start
      && locals->address <= locals->curr_end - 1)
    {
      locals->vma->start = locals->curr_start;
      locals->vma->end = locals->curr_end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  locals->curr_start = start; locals->curr_end = end;
  return 0;
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct callback_locals locals;
  locals.address = address;
  locals.vma = vma;
  locals.curr_start = 0;
  locals.curr_end = 0;
# if STACK_DIRECTION < 0
  locals.prev_end = 0;
# else
  locals.stop_at_next_vma = 0;
# endif
  locals.retval = -1;

  vma_iterate (&locals);
  if (locals.retval < 0)
    {
      if (locals.curr_start < locals.curr_end
          && address >= locals.curr_start && address <= locals.curr_end - 1)
        {
          vma->start = locals.curr_start;
          vma->end = locals.curr_end;
# if STACK_DIRECTION < 0
          vma->prev_end = locals.prev_end;
# else
          vma->next_start = 0;
# endif
          locals.retval = 0;
        }
    }
  if (locals.retval == 0)
    {
# if !(STACK_DIRECTION < 0)
      if (locals.stop_at_next_vma)
        vma->next_start = 0;
# endif
      vma->is_near_this = simple_is_near_this;
      return 0;
    }

  return mincore_get_vma (address, vma);
}

/* --------------------------- stackvma-mquery.c --------------------------- */

/* mquery() is a system call that allows to inquire the status of a
   range of pages of virtual memory.  In particular, it allows to inquire
   whether a page is mapped at all, and where is the next unmapped page
   after a given address.
   As of 2021, mquery() is supported by:
     - OpenBSD, since OpenBSD 3.4.
   Note that this file can give different results.  For example, on
   OpenBSD 4.4 / i386 the stack segment (which starts around 0xcdbfe000)
   ends at 0xcfbfdfff according to mincore, but at 0xffffffff according to
   mquery.  */

#elif defined __OpenBSD__ /* OpenBSD */

# include <unistd.h> /* getpagesize, mincore */
# include <sys/types.h>
# include <sys/mman.h> /* mincore */

/* Cache for getpagesize().  */
static uintptr_t pagesize;

/* Initialize pagesize.  */
static void
init_pagesize (void)
{
  pagesize = getpagesize ();
}

/* Test whether the page starting at ADDR is among the address range.
   ADDR must be a multiple of pagesize.  */
static int
is_mapped (uintptr_t addr)
{
  /* Avoid calling mquery with a NULL first argument, because this argument
     value has a specific meaning.  We know the NULL page is unmapped.  */
  if (addr == 0)
    return 0;
  return mquery ((void *) addr, pagesize, 0, MAP_FIXED, -1, 0) == (void *) -1;
}

/* Assuming that the page starting at ADDR is among the address range,
   return the start of its virtual memory range.
   ADDR must be a multiple of pagesize.  */
static uintptr_t
mapped_range_start (uintptr_t addr)
{
  uintptr_t stepsize;
  uintptr_t known_unmapped_page;

  /* Look at smaller addresses, in larger and larger steps, to minimize the
     number of mquery() calls.  */
  stepsize = pagesize;
  for (;;)
    {
      uintptr_t hole;

      if (addr == 0)
        abort ();

      if (addr <= stepsize)
        {
          known_unmapped_page = 0;
          break;
        }

      hole = (uintptr_t) mquery ((void *) (addr - stepsize), pagesize,
                                     0, 0, -1, 0);
      if (!(hole == (uintptr_t) (void *) -1 || hole >= addr))
        {
          /* Some part of [addr - stepsize, addr - 1] is unmapped.  */
          known_unmapped_page = hole;
          break;
        }

      /* The entire range [addr - stepsize, addr - 1] is mapped.  */
      addr -= stepsize;

      if (2 * stepsize > stepsize && 2 * stepsize < addr)
        stepsize = 2 * stepsize;
    }

  /* Now reduce the step size again.
     We know that the page at known_unmapped_page is unmapped and that
     0 < addr - known_unmapped_page <= stepsize.  */
  while (stepsize > pagesize && stepsize / 2 >= addr - known_unmapped_page)
    stepsize = stepsize / 2;
  /* Still 0 < addr - known_unmapped_page <= stepsize.  */
  while (stepsize > pagesize)
    {
      uintptr_t hole;

      stepsize = stepsize / 2;
      hole = (uintptr_t) mquery ((void *) (addr - stepsize), pagesize,
                                     0, 0, -1, 0);
      if (!(hole == (uintptr_t) (void *) -1 || hole >= addr))
        /* Some part of [addr - stepsize, addr - 1] is unmapped.  */
        known_unmapped_page = hole;
      else
        /* The entire range [addr - stepsize, addr - 1] is mapped.  */
        addr -= stepsize;
      /* Still 0 < addr - known_unmapped_page <= stepsize.  */
    }

  return addr;
}

/* Assuming that the page starting at ADDR is among the address range,
   return the end of its virtual memory range + 1.
   ADDR must be a multiple of pagesize.  */
static uintptr_t
mapped_range_end (uintptr_t addr)
{
  uintptr_t end;

  if (addr == 0)
    abort ();

  end = (uintptr_t) mquery ((void *) addr, pagesize, 0, 0, -1, 0);
  if (end == (uintptr_t) (void *) -1)
    end = 0; /* wrap around */
  return end;
}

/* Determine whether an address range [ADDR1..ADDR2] is completely unmapped.
   ADDR1 must be <= ADDR2.  */
static int
is_unmapped (uintptr_t addr1, uintptr_t addr2)
{
  /* Round addr1 down.  */
  addr1 = (addr1 / pagesize) * pagesize;
  /* Round addr2 up and turn it into an exclusive bound.  */
  addr2 = ((addr2 / pagesize) + 1) * pagesize;

  /* Avoid calling mquery with a NULL first argument, because this argument
     value has a specific meaning.  We know the NULL page is unmapped.  */
  if (addr1 == 0)
    addr1 = pagesize;

  if (addr1 < addr2)
    {
      if (mquery ((void *) addr1, addr2 - addr1, 0, MAP_FIXED, -1, 0)
          == (void *) -1)
        /* Not all the interval [addr1 .. addr2 - 1] is unmapped.  */
        return 0;
      else
        /* The interval [addr1 .. addr2 - 1] is unmapped.  */
        return 1;
    }
  return 1;
}

# if STACK_DIRECTION < 0

/* Info about the gap between this VMA and the previous one.
   addr must be < vma->start.  */
static int
mquery_is_near_this (uintptr_t addr, struct vma_struct *vma)
{
  /*   vma->start - addr <= (vma->start - vma->prev_end) / 2
     is mathematically equivalent to
       vma->prev_end <= 2 * addr - vma->start
     <==> is_unmapped (2 * addr - vma->start, vma->start - 1).
     But be careful about overflow: if 2 * addr - vma->start is negative,
     we consider a tiny "guard page" mapping [0, 0] to be present around
     NULL; it intersects the range (2 * addr - vma->start, vma->start - 1),
     therefore return false.  */
  uintptr_t testaddr = addr - (vma->start - addr);
  if (testaddr > addr) /* overflow? */
    return 0;
  /* Here testaddr <= addr < vma->start.  */
  return is_unmapped (testaddr, vma->start - 1);
}

# endif
# if STACK_DIRECTION > 0

/* Info about the gap between this VMA and the next one.
   addr must be > vma->end - 1.  */
static int
mquery_is_near_this (uintptr_t addr, struct vma_struct *vma)
{
  /*   addr - vma->end < (vma->next_start - vma->end) / 2
     is mathematically equivalent to
       vma->next_start > 2 * addr - vma->end
     <==> is_unmapped (vma->end, 2 * addr - vma->end).
     But be careful about overflow: if 2 * addr - vma->end is > ~0UL,
     we consider a tiny "guard page" mapping [0, 0] to be present around
     NULL; it intersects the range (vma->end, 2 * addr - vma->end),
     therefore return false.  */
  uintptr_t testaddr = addr + (addr - vma->end);
  if (testaddr < addr) /* overflow? */
    return 0;
  /* Here vma->end - 1 < addr <= testaddr.  */
  return is_unmapped (vma->end, testaddr);
}

# endif

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  if (pagesize == 0)
    init_pagesize ();
  address = (address / pagesize) * pagesize;
  vma->start = mapped_range_start (address);
  vma->end = mapped_range_end (address);
  vma->is_near_this = mquery_is_near_this;
  return 0;
}

/* ---------------------------- stackvma-mach.c ---------------------------- */

#elif (defined __APPLE__ && defined __MACH__) /* macOS */

#include <libc.h>
#include <nlist.h>
#include <mach/mach.h>
#include <mach/machine/vm_param.h>

int
sigsegv_get_vma (uintptr_t req_address, struct vma_struct *vma)
{
  uintptr_t prev_address = 0, prev_size = 0;
  uintptr_t join_address = 0, join_size = 0;
  int more = 1;
  vm_address_t address;
  vm_size_t size;
  task_t task = mach_task_self ();

  for (address = VM_MIN_ADDRESS; more; address += size)
    {
      mach_port_t object_name;
      /* In MacOS X 10.5, the types vm_address_t, vm_offset_t, vm_size_t have
         32 bits in 32-bit processes and 64 bits in 64-bit processes. Whereas
         mach_vm_address_t and mach_vm_size_t are always 64 bits large.
         MacOS X 10.5 has three vm_region like methods:
           - vm_region. It has arguments that depend on whether the current
             process is 32-bit or 64-bit. When linking dynamically, this
             function exists only in 32-bit processes. Therefore we use it only
             in 32-bit processes.
           - vm_region_64. It has arguments that depend on whether the current
             process is 32-bit or 64-bit. It interprets a flavor
             VM_REGION_BASIC_INFO as VM_REGION_BASIC_INFO_64, which is
             dangerous since 'struct vm_region_basic_info_64' is larger than
             'struct vm_region_basic_info'; therefore let's write
             VM_REGION_BASIC_INFO_64 explicitly.
           - mach_vm_region. It has arguments that are 64-bit always. This
             function is useful when you want to access the VM of a process
             other than the current process.
         In 64-bit processes, we could use vm_region_64 or mach_vm_region.
         I choose vm_region_64 because it uses the same types as vm_region,
         resulting in less conditional code.  */
# if defined __aarch64__ || defined __ppc64__ || defined __x86_64__
      struct vm_region_basic_info_64 info;
      mach_msg_type_number_t info_count = VM_REGION_BASIC_INFO_COUNT_64;

      more = (vm_region_64 (task, &address, &size, VM_REGION_BASIC_INFO_64,
                            (vm_region_info_t)&info, &info_count, &object_name)
              == KERN_SUCCESS);
# else
      struct vm_region_basic_info info;
      mach_msg_type_number_t info_count = VM_REGION_BASIC_INFO_COUNT;

      more = (vm_region (task, &address, &size, VM_REGION_BASIC_INFO,
                         (vm_region_info_t)&info, &info_count, &object_name)
              == KERN_SUCCESS);
# endif
      if (!more)
        {
          address = join_address + join_size;
          size = 0;
        }

      if ((uintptr_t) address == join_address + join_size)
        join_size += size;
      else
        {
          prev_address = join_address;
          prev_size = join_size;
          join_address = (uintptr_t) address;
          join_size = size;
        }

      if (object_name != MACH_PORT_NULL)
        mach_port_deallocate (mach_task_self (), object_name);

# if STACK_DIRECTION < 0
      if (join_address <= req_address && join_address + join_size > req_address)
        {
          vma->start = join_address;
          vma->end = join_address + join_size;
          vma->prev_end = prev_address + prev_size;
          vma->is_near_this = simple_is_near_this;
          return 0;
        }
# else
      if (prev_address <= req_address && prev_address + prev_size > req_address)
        {
          vma->start = prev_address;
          vma->end = prev_address + prev_size;
          vma->next_start = join_address;
          vma->is_near_this = simple_is_near_this;
          return 0;
        }
# endif
    }

# if STACK_DIRECTION > 0
  if (join_address <= req_address && join_address + size > req_address)
    {
      vma->start = prev_address;
      vma->end = prev_address + prev_size;
      vma->next_start = ~0UL;
      vma->is_near_this = simple_is_near_this;
      return 0;
    }
# endif

  return -1;
}

/* ----------------------------- stackvma-aix.c ----------------------------- */

#elif defined _AIX /* AIX */

# include <unistd.h> /* getpagesize, getpid, close, read */
# include <errno.h> /* EINTR */
# include <fcntl.h> /* open */
# include <string.h> /* memcpy */
# include <sys/types.h>
# include <sys/mman.h> /* mmap, munmap */
# include <sys/procfs.h> /* prmap_t */
# include <sys/utsname.h> /* uname */

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
# if STACK_DIRECTION < 0
  uintptr_t prev;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
# if STACK_DIRECTION < 0
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->vma->prev_end = locals->prev;
      locals->retval = 0;
      return 1;
    }
  locals->prev = end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  return 0;
}

/* Iterate over the virtual memory areas of the current process.
   If such iteration is supported, the callback is called once for every
   virtual memory area, in ascending order, with the following arguments:
     - LOCALS is the same argument as passed to vma_iterate.
     - START is the address of the first byte in the area, page-aligned.
     - END is the address of the last byte in the area plus 1, page-aligned.
       Note that it may be 0 for the last area in the address space.
   If the callback returns 0, the iteration continues.  If it returns 1,
   the iteration terminates prematurely.
   This function may open file descriptors, but does not call malloc().
   Return 0 if all went well, or -1 in case of error.  */
/* This code is a simplified copy (no handling of protection flags) of the
   code in gnulib's lib/vma-iter.c.  */
static int
vma_iterate (struct callback_locals *locals)
{
  /* On AIX, there is a /proc/$pic/map file, that contains records of type
     prmap_t, defined in <sys/procfs.h>.  In older versions of AIX, it lists
     only the virtual memory areas that are connected to a file, not the
     anonymous ones.  But at least since AIX 7.1, it is well usable.  */

  char fnamebuf[6+10+4+1];
  char *fname;
  int fd;
  size_t memneed;

  if (pagesize == 0)
    init_pagesize ();

  /* Construct fname = sprintf (fnamebuf+i, "/proc/%u/map", getpid ()).  */
  fname = fnamebuf + sizeof (fnamebuf) - (4+1);
  memcpy (fname, "/map", 4+1);
  {
    unsigned int value = getpid ();
    do
      *--fname = (value % 10) + '0';
    while ((value = value / 10) > 0);
  }
  fname -= 6;
  memcpy (fname, "/proc/", 6);

  fd = open (fname, O_RDONLY | O_CLOEXEC);
  if (fd < 0)
    return -1;

  /* The contents of /proc/<pid>/map contains a number of prmap_t entries,
     then an entirely null prmap_t entry, then a heap of NUL terminated
     strings.
     Documentation: https://www.ibm.com/docs/en/aix/7.1?topic=files-proc-file
     We read the entire contents, but look only at the prmap_t entries and
     ignore the tail part.  */

  for (memneed = 2 * pagesize; ; memneed = 2 * memneed)
    {
      /* Allocate memneed bytes of memory.
         We cannot use alloca here, because not much stack space is guaranteed.
         We also cannot use malloc here, because a malloc() call may call mmap()
         and thus pre-allocate available memory.
         So use mmap(), and ignore the resulting VMA if it occurs among the
         resulting VMAs.  (Normally it doesn't, because it was allocated after
         the open() call.)  */
      void *auxmap;
      unsigned long auxmap_start;
      unsigned long auxmap_end;
      ssize_t nbytes;

      auxmap = (void *) mmap ((void *) 0, memneed, PROT_READ | PROT_WRITE,
                              MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
      if (auxmap == (void *) -1)
        {
          close (fd);
          return -1;
        }
      auxmap_start = (unsigned long) auxmap;
      auxmap_end = auxmap_start + memneed;

      /* Read the contents of /proc/<pid>/map in a single system call.
         This guarantees a consistent result (no duplicated or omitted
         entries).  */
     retry:
      do
        nbytes = read (fd, auxmap, memneed);
      while (nbytes < 0 && errno == EINTR);
      if (nbytes <= 0)
        {
          munmap (auxmap, memneed);
          close (fd);
          return -1;
        }
      if (nbytes == memneed)
        {
          /* Need more memory.  */
          munmap (auxmap, memneed);
          if (lseek (fd, 0, SEEK_SET) < 0)
            {
              close (fd);
              return -1;
            }
        }
      else
        {
          if (read (fd, (char *) auxmap + nbytes, 1) > 0)
            {
              /* Oops, we had a short read.  Retry.  */
              if (lseek (fd, 0, SEEK_SET) < 0)
                {
                  munmap (auxmap, memneed);
                  close (fd);
                  return -1;
                }
              goto retry;
            }

          /* We now have the entire contents of /proc/<pid>/map in memory.  */
          prmap_t* maps = (prmap_t *) auxmap;

          /* The entries are not sorted by address.  Therefore
             1. Extract the relevant information into an array.
             2. Sort the array in ascending order.
             3. Invoke the callback.  */
          typedef struct
            {
              uintptr_t start;
              uintptr_t end;
            }
          vma_t;
          /* Since 2 * sizeof (vma_t) <= sizeof (prmap_t), we can reuse the
             same memory.  */
          vma_t *vmas = (vma_t *) auxmap;

          vma_t *vp = vmas;
          {
            prmap_t* mp;
            for (mp = maps;;)
              {
                unsigned long start, end;

                start = (unsigned long) mp->pr_vaddr;
                end = start + mp->pr_size;
                if (start == 0 && end == 0 && mp->pr_mflags == 0)
                  break;
                /* Discard empty VMAs and kernel VMAs.  */
                if (start < end && (mp->pr_mflags & MA_KERNTEXT) == 0)
                  {
                    if (start <= auxmap_start && auxmap_end - 1 <= end - 1)
                      {
                        /* Consider [start,end-1] \ [auxmap_start,auxmap_end-1]
                           = [start,auxmap_start-1] u [auxmap_end,end-1].  */
                        if (start < auxmap_start)
                          {
                            vp->start = start;
                            vp->end = auxmap_start;
                            vp++;
                          }
                        if (auxmap_end - 1 < end - 1)
                          {
                            vp->start = auxmap_end;
                            vp->end = end;
                            vp++;
                          }
                      }
                    else
                      {
                        vp->start = start;
                        vp->end = end;
                        vp++;
                      }
                  }
                mp++;
              }
          }

          size_t nvmas = vp - vmas;
          /* Sort the array in ascending order.
             Better not call qsort(), since it may call malloc().
             Insertion-sort is OK in this case, despite its worst-case running
             time of O(N²), since the number of VMAs will rarely be larger than
             1000.  */
          {
            size_t i;
            for (i = 1; i < nvmas; i++)
              {
                /* Invariant: Here vmas[0..i-1] is sorted.  */
                size_t j;
                for (j = i; j > 0 && vmas[j - 1].start > vmas[j].start; j--)
                  {
                    vma_t tmp = vmas[j - 1];
                    vmas[j - 1] = vmas[j];
                    vmas[j] = tmp;
                  }
                /* Invariant: Here vmas[0..i] is sorted.  */
              }
          }

          /* Invoke the callback.  */
          {
            size_t i;
            for (i = 0; i < nvmas; i++)
              {
                vma_t *vpi = &vmas[i];
                if (callback (locals, vpi->start, vpi->end))
                  break;
              }
          }

          munmap (auxmap, memneed);
          break;
        }
    }

  close (fd);
  return 0;
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct utsname u;
  if (uname (&u) >= 0
      /* && strcmp (u.sysname, "AIX") == 0 */
      && !(u.version[0] >= '1' && u.version[0] <= '6' && u.version[1] == '\0'))
    {
      /* AIX 7 or higher.  */
      struct callback_locals locals;
      locals.address = address;
      locals.vma = vma;
#if STACK_DIRECTION < 0
      locals.prev = 0;
#else
      locals.stop_at_next_vma = 0;
#endif
      locals.retval = -1;

      vma_iterate (&locals);
      if (locals.retval == 0)
        {
#if !(STACK_DIRECTION < 0)
          if (locals.stop_at_next_vma)
            vma->next_start = 0;
#endif
          vma->is_near_this = simple_is_near_this;
          return 0;
        }
    }

  return mincore_get_vma (address, vma);
}

/* --------------------------- stackvma-procfs.c --------------------------- */

#elif defined __sgi || defined __sun /* IRIX, Solaris */

# include <errno.h> /* errno, EINTR */
# include <fcntl.h> /* open, O_RDONLY */
# include <stddef.h> /* size_t */
# include <unistd.h> /* getpagesize, getpid, read, close */
# include <sys/types.h>
# include <sys/mman.h> /* mmap, munmap */
# include <sys/stat.h> /* fstat */
# include <string.h> /* memcpy */

/* Try to use the newer ("structured") /proc filesystem API, if supported.  */
# define _STRUCTURED_PROC 1
# include <sys/procfs.h> /* prmap_t, optionally PIOC* */

# if !defined __sun

/* Cache for getpagesize().  */
static uintptr_t pagesize;

/* Initialize pagesize.  */
static void
init_pagesize (void)
{
  pagesize = getpagesize ();
}

# endif

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
# if STACK_DIRECTION < 0
  uintptr_t prev;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
# if STACK_DIRECTION < 0
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->vma->prev_end = locals->prev;
      locals->retval = 0;
      return 1;
    }
  locals->prev = end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  return 0;
}

/* Iterate over the virtual memory areas of the current process.
   If such iteration is supported, the callback is called once for every
   virtual memory area, in ascending order, with the following arguments:
     - LOCALS is the same argument as passed to vma_iterate.
     - START is the address of the first byte in the area, page-aligned.
     - END is the address of the last byte in the area plus 1, page-aligned.
       Note that it may be 0 for the last area in the address space.
   If the callback returns 0, the iteration continues.  If it returns 1,
   the iteration terminates prematurely.
   This function may open file descriptors, but does not call malloc().
   Return 0 if all went well, or -1 in case of error.  */
/* This code is a simplified copy (no handling of protection flags) of the
   code in gnulib's lib/vma-iter.c.  */
static int
vma_iterate (struct callback_locals *locals)
{
  /* Note: Solaris <sys/procfs.h> defines a different type prmap_t with
     _STRUCTURED_PROC than without! Here's a table of sizeof(prmap_t):
                                  32-bit   64-bit
         _STRUCTURED_PROC = 0       32       56
         _STRUCTURED_PROC = 1       96      104
     Therefore, if the include files provide the newer API, prmap_t has
     the bigger size, and thus you MUST use the newer API.  And if the
     include files provide the older API, prmap_t has the smaller size,
     and thus you MUST use the older API.  */

# if defined PIOCNMAP && defined PIOCMAP
  /* We must use the older /proc interface.  */

  char fnamebuf[6+10+1];
  char *fname;
  int fd;
  int nmaps;
  size_t memneed;
#  if HAVE_MAP_ANONYMOUS
#   define zero_fd -1
#   define map_flags MAP_ANONYMOUS
#  else /* !HAVE_MAP_ANONYMOUS */
  int zero_fd;
#   define map_flags 0
#  endif
  void *auxmap;
  uintptr_t auxmap_start;
  uintptr_t auxmap_end;
  prmap_t* maps;
  prmap_t* mp;

  if (pagesize == 0)
    init_pagesize ();

  /* Construct fname = sprintf (fnamebuf+i, "/proc/%u", getpid ()).  */
  fname = fnamebuf + sizeof (fnamebuf) - 1;
  *fname = '\0';
  {
    unsigned int value = getpid ();
    do
      *--fname = (value % 10) + '0';
    while ((value = value / 10) > 0);
  }
  fname -= 6;
  memcpy (fname, "/proc/", 6);

  fd = open (fname, O_RDONLY);
  if (fd < 0)
    return -1;

  if (ioctl (fd, PIOCNMAP, &nmaps) < 0)
    goto fail2;

  memneed = (nmaps + 10) * sizeof (prmap_t);
  /* Allocate memneed bytes of memory.
     We cannot use alloca here, because not much stack space is guaranteed.
     We also cannot use malloc here, because a malloc() call may call mmap()
     and thus pre-allocate available memory.
     So use mmap(), and ignore the resulting VMA.  */
  memneed = ((memneed - 1) / pagesize + 1) * pagesize;
#  if !HAVE_MAP_ANONYMOUS
  zero_fd = open ("/dev/zero", O_RDONLY, 0644);
  if (zero_fd < 0)
    goto fail2;
#  endif
  auxmap = (void *) mmap ((void *) 0, memneed, PROT_READ | PROT_WRITE,
                          map_flags | MAP_PRIVATE, zero_fd, 0);
#  if !HAVE_MAP_ANONYMOUS
  close (zero_fd);
#  endif
  if (auxmap == (void *) -1)
    goto fail2;
  auxmap_start = (uintptr_t) auxmap;
  auxmap_end = auxmap_start + memneed;
  maps = (prmap_t *) auxmap;

  if (ioctl (fd, PIOCMAP, maps) < 0)
    goto fail1;

  for (mp = maps;;)
    {
      uintptr_t start, end;

      start = (uintptr_t) mp->pr_vaddr;
      end = start + mp->pr_size;
      if (start == 0 && end == 0)
        break;
      mp++;
      if (start <= auxmap_start && auxmap_end - 1 <= end - 1)
        {
          /* Consider [start,end-1] \ [auxmap_start,auxmap_end-1]
             = [start,auxmap_start-1] u [auxmap_end,end-1].  */
          if (start < auxmap_start)
            if (callback (locals, start, auxmap_start))
              break;
          if (auxmap_end - 1 < end - 1)
            if (callback (locals, auxmap_end, end))
              break;
        }
      else
        {
          if (callback (locals, start, end))
            break;
        }
    }
  munmap (auxmap, memneed);
  close (fd);
  return 0;

 fail1:
  munmap (auxmap, memneed);
 fail2:
  close (fd);
  return -1;

# else
  /* We must use the newer /proc interface.
     Documentation:
     https://docs.oracle.com/cd/E23824_01/html/821-1473/proc-4.html
     The contents of /proc/<pid>/map consists of records of type
     prmap_t.  These are different in 32-bit and 64-bit processes,
     but here we are fortunately accessing only the current process.  */

  char fnamebuf[6+10+4+1];
  char *fname;
  int fd;
  int nmaps;
  size_t memneed;
#  if HAVE_MAP_ANONYMOUS
#   define zero_fd -1
#   define map_flags MAP_ANONYMOUS
#  else /* !HAVE_MAP_ANONYMOUS */
  int zero_fd;
#   define map_flags 0
#  endif
  void *auxmap;
  uintptr_t auxmap_start;
  uintptr_t auxmap_end;
  prmap_t* maps;
  prmap_t* maps_end;
  prmap_t* mp;

  if (pagesize == 0)
    init_pagesize ();

  /* Construct fname = sprintf (fnamebuf+i, "/proc/%u/map", getpid ()).  */
  fname = fnamebuf + sizeof (fnamebuf) - 1 - 4;
  memcpy (fname, "/map", 4 + 1);
  {
    unsigned int value = getpid ();
    do
      *--fname = (value % 10) + '0';
    while ((value = value / 10) > 0);
  }
  fname -= 6;
  memcpy (fname, "/proc/", 6);

  fd = open (fname, O_RDONLY);
  if (fd < 0)
    return -1;

  {
    struct stat statbuf;
    if (fstat (fd, &statbuf) < 0)
      goto fail2;
    nmaps = statbuf.st_size / sizeof (prmap_t);
  }

  memneed = (nmaps + 10) * sizeof (prmap_t);
  /* Allocate memneed bytes of memory.
     We cannot use alloca here, because not much stack space is guaranteed.
     We also cannot use malloc here, because a malloc() call may call mmap()
     and thus pre-allocate available memory.
     So use mmap(), and ignore the resulting VMA.  */
  memneed = ((memneed - 1) / pagesize + 1) * pagesize;
#  if !HAVE_MAP_ANONYMOUS
  zero_fd = open ("/dev/zero", O_RDONLY, 0644);
  if (zero_fd < 0)
    goto fail2;
#  endif
  auxmap = (void *) mmap ((void *) 0, memneed, PROT_READ | PROT_WRITE,
                          map_flags | MAP_PRIVATE, zero_fd, 0);
#  if !HAVE_MAP_ANONYMOUS
  close (zero_fd);
#  endif
  if (auxmap == (void *) -1)
    goto fail2;
  auxmap_start = (uintptr_t) auxmap;
  auxmap_end = auxmap_start + memneed;
  maps = (prmap_t *) auxmap;

  /* Read up to memneed bytes from fd into maps.  */
  {
    size_t remaining = memneed;
    size_t total_read = 0;
    char *ptr = (char *) maps;

    do
      {
        size_t nread = read (fd, ptr, remaining);
        if (nread == (size_t)-1)
          {
            if (errno == EINTR)
              continue;
            goto fail1;
          }
        if (nread == 0)
          /* EOF */
          break;
        total_read += nread;
        ptr += nread;
        remaining -= nread;
      }
    while (remaining > 0);

    nmaps = (memneed - remaining) / sizeof (prmap_t);
    maps_end = maps + nmaps;
  }

  for (mp = maps; mp < maps_end; mp++)
    {
      uintptr_t start, end;

      start = (uintptr_t) mp->pr_vaddr;
      end = start + mp->pr_size;
      if (start <= auxmap_start && auxmap_end - 1 <= end - 1)
        {
          /* Consider [start,end-1] \ [auxmap_start,auxmap_end-1]
             = [start,auxmap_start-1] u [auxmap_end,end-1].  */
          if (start < auxmap_start)
            if (callback (locals, start, auxmap_start))
              break;
          if (auxmap_end - 1 < end - 1)
            if (callback (locals, auxmap_end, end))
              break;
        }
      else
        {
          if (callback (locals, start, end))
            break;
        }
    }
  munmap (auxmap, memneed);
  close (fd);
  return 0;

 fail1:
  munmap (auxmap, memneed);
 fail2:
  close (fd);
  return -1;

# endif
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct callback_locals locals;
  locals.address = address;
  locals.vma = vma;
# if STACK_DIRECTION < 0
  locals.prev = 0;
# else
  locals.stop_at_next_vma = 0;
# endif
  locals.retval = -1;

  vma_iterate (&locals);
  if (locals.retval == 0)
    {
# if !(STACK_DIRECTION < 0)
      if (locals.stop_at_next_vma)
        vma->next_start = 0;
# endif
      vma->is_near_this = simple_is_near_this;
      return 0;
    }

# if defined __sun
  return mincore_get_vma (address, vma);
# else
  return -1;
# endif
}

/* -------------------------------------------------------------------------- */

#elif defined __CYGWIN__ /* Cygwin */

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
  /* The stack appears as three adjacent segments, therefore we
     merge adjacent segments.  */
  uintptr_t curr_start, curr_end;
# if STACK_DIRECTION < 0
  uintptr_t prev_end;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
  if (start == locals->curr_end)
    {
      /* Merge adjacent segments.  */
      locals->curr_end = end;
      return 0;
    }
# if STACK_DIRECTION < 0
  if (locals->curr_start < locals->curr_end
      && locals->address >= locals->curr_start
      && locals->address <= locals->curr_end - 1)
    {
      locals->vma->start = locals->curr_start;
      locals->vma->end = locals->curr_end;
      locals->vma->prev_end = locals->prev_end;
      locals->retval = 0;
      return 1;
    }
  locals->prev_end = locals->curr_end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = locals->curr_start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->curr_start < locals->curr_end
      && locals->address >= locals->curr_start
      && locals->address <= locals->curr_end - 1)
    {
      locals->vma->start = locals->curr_start;
      locals->vma->end = locals->curr_end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  locals->curr_start = start; locals->curr_end = end;
  return 0;
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct callback_locals locals;
  locals.address = address;
  locals.vma = vma;
  locals.curr_start = 0;
  locals.curr_end = 0;
# if STACK_DIRECTION < 0
  locals.prev_end = 0;
# else
  locals.stop_at_next_vma = 0;
# endif
  locals.retval = -1;

  vma_iterate (&locals);
  if (locals.retval < 0)
    {
      if (locals.curr_start < locals.curr_end
          && address >= locals.curr_start && address <= locals.curr_end - 1)
        {
          vma->start = locals.curr_start;
          vma->end = locals.curr_end;
# if STACK_DIRECTION < 0
          vma->prev_end = locals.prev_end;
# else
          vma->next_start = 0;
# endif
          locals.retval = 0;
        }
    }
  if (locals.retval == 0)
    {
# if !(STACK_DIRECTION < 0)
      if (locals.stop_at_next_vma)
        vma->next_start = 0;
# endif
      vma->is_near_this = simple_is_near_this;
      return 0;
    }

  return -1;
}

/* ---------------------------- stackvma-beos.h ---------------------------- */

#elif defined __HAIKU__ /* Haiku */

# include <OS.h> /* get_next_area_info */

struct callback_locals
{
  uintptr_t address;
  struct vma_struct *vma;
# if STACK_DIRECTION < 0
  uintptr_t prev;
# else
  int stop_at_next_vma;
# endif
  int retval;
};

static int
callback (struct callback_locals *locals, uintptr_t start, uintptr_t end)
{
# if STACK_DIRECTION < 0
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->vma->prev_end = locals->prev;
      locals->retval = 0;
      return 1;
    }
  locals->prev = end;
# else
  if (locals->stop_at_next_vma)
    {
      locals->vma->next_start = start;
      locals->stop_at_next_vma = 0;
      return 1;
    }
  if (locals->address >= start && locals->address <= end - 1)
    {
      locals->vma->start = start;
      locals->vma->end = end;
      locals->retval = 0;
      locals->stop_at_next_vma = 1;
      return 0;
    }
# endif
  return 0;
}

/* Iterate over the virtual memory areas of the current process.
   If such iteration is supported, the callback is called once for every
   virtual memory area, in ascending order, with the following arguments:
     - LOCALS is the same argument as passed to vma_iterate.
     - START is the address of the first byte in the area, page-aligned.
     - END is the address of the last byte in the area plus 1, page-aligned.
       Note that it may be 0 for the last area in the address space.
   If the callback returns 0, the iteration continues.  If it returns 1,
   the iteration terminates prematurely.
   This function may open file descriptors, but does not call malloc().
   Return 0 if all went well, or -1 in case of error.  */
/* This code is a simplified copy (no handling of protection flags) of the
   code in gnulib's lib/vma-iter.c.  */
static int
vma_iterate (struct callback_locals *locals)
{
  area_info info;
  ssize_t cookie;

  cookie = 0;
  while (get_next_area_info (0, &cookie, &info) == B_OK)
    {
      uintptr_t start, end;

      start = (uintptr_t) info.address;
      end = start + info.size;

      if (callback (locals, start, end))
        break;
    }
  return 0;
}

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  struct callback_locals locals;
  locals.address = address;
  locals.vma = vma;
# if STACK_DIRECTION < 0
  locals.prev = 0;
# else
  locals.stop_at_next_vma = 0;
# endif
  locals.retval = -1;

  vma_iterate (&locals);
  if (locals.retval == 0)
    {
# if !(STACK_DIRECTION < 0)
      if (locals.stop_at_next_vma)
        vma->next_start = 0;
# endif
      vma->is_near_this = simple_is_near_this;
      return 0;
    }
  return -1;
}

/* -------------------------------------------------------------------------- */

#else /* Hurd, Minix, ... */

int
sigsegv_get_vma (uintptr_t address, struct vma_struct *vma)
{
  /* No way.  */
  return -1;
}

#endif
