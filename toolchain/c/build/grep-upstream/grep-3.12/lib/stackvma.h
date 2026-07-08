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

#ifndef _STACKVMA_H
#define _STACKVMA_H

#include <stdint.h>

/* Describes a virtual memory area, with some info about the gap between
   it and the next or previous virtual memory area.  */
struct vma_struct
{
  uintptr_t start;
  uintptr_t end;
#if STACK_DIRECTION < 0
  /* Info about the gap between this VMA and the previous one.
     addr must be < vma->start.  */
  int (*is_near_this) (uintptr_t addr, struct vma_struct *vma);
  /* Private field, not provided by all sigsegv_get_vma implementations.  */
  uintptr_t prev_end;
#endif
#if STACK_DIRECTION > 0
  /* Info about the gap between this VMA and the next one.
     addr must be > vma->end - 1.  */
  int (*is_near_this) (uintptr_t addr, struct vma_struct *vma);
  /* Private field, not provided by all sigsegv_get_vma implementations.  */
  uintptr_t next_start;
#endif
};

/* Determines the virtual memory area to which a given address belongs,
   and returns 0.  Returns -1 if it cannot be determined.
   This function is used to determine the stack extent when a fault occurs.  */
extern int sigsegv_get_vma (uintptr_t address, struct vma_struct *vma);

/* Defined if sigsegv_get_vma actually works (i.e. does not always fail).  */
#if defined __linux__ || defined __ANDROID__ \
    || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
    || defined __NetBSD__ || defined __OpenBSD__ \
    || (defined __APPLE__ && defined __MACH__) \
    || defined _AIX || defined __sgi || defined __sun \
    || defined __CYGWIN__ || defined __HAIKU__
# define HAVE_STACKVMA 1
#endif

#endif /* _STACKVMA_H */
