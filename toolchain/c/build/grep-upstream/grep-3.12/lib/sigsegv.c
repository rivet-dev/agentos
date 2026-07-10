/* Page fault handling library.
   Copyright (C) 1993-2025 Free Software Foundation, Inc.
   Copyright (C) 2018  Nylon Chen <nylon7@andestech.com>

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

/* Written by Bruno Haible and Nylon Chen.  */

#include <config.h>

/* Specification.  */
#include "sigsegv.h"

#include <errno.h>
#include <stdio.h> /* declares perror */
#include <stdint.h> /* defines uintptr_t */
#include <stdlib.h>
#include <signal.h>
#if HAVE_GETRLIMIT
# include <sys/resource.h> /* declares struct rlimit */
#endif

#ifdef __OpenBSD__
# include <sys/param.h> /* defines macro OpenBSD */
#endif


/* Version number.  */
int libsigsegv_version = LIBSIGSEGV_VERSION;


/* ======================= Fault handler information ======================= */

/* Define:

     SIGSEGV_FAULT_HANDLER_ARGLIST
          is the argument list for the actual fault handler.

   and if available (optional):

     SIGSEGV_FAULT_ADDRESS
          is a macro for fetching the fault address.

     SIGSEGV_FAULT_CONTEXT
          is a macro giving a pointer to the entire fault context (i.e.
          the register set etc.).

     SIGSEGV_FAULT_STACKPOINTER
          is a macro for fetching the stackpointer at the moment the fault
          occurred.
 */

#if defined __linux__ && !defined __ANDROID__ /* Linux */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

# if defined __alpha__

/* See glibc/sysdeps/unix/sysv/linux/alpha/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/alpha/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/alpha/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are actually the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.sc_regs[30]

# elif defined __arm64__ /* 64-bit */

/* See glibc/sysdeps/unix/sysv/linux/aarch64/sys/ucontext.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/aarch64/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are actually the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.sp

# elif defined __arm__ || defined __armhf__ /* 32-bit */

/* See glibc/sysdeps/unix/sysv/linux/arm/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/arm/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/arm/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are actually the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.arm_sp

# elif defined __cris__

/* See glibc-ports/sysdeps/unix/sysv/linux/cris/sys/ucontext.h.
   Note that the 'mcontext_t' defined in
   glibc-ports/sysdeps/unix/sysv/linux/cris/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are actually the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.usp

# elif defined __hppa__

/* See glibc/sysdeps/unix/sysv/linux/hppa/sys/ucontext.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/hppa/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are actually the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.sc_gr[30]

# elif defined __x86_64__ /* 64 bit registers */

/* See glibc/sysdeps/unix/sysv/linux/x86/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/x86_64/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/x86/sys/ucontext.h
   and the 'struct sigcontext' defined in
   glibc/sysdeps/unix/sysv/linux/x86/bits/sigcontext.h
   (see also <asm/sigcontext.h>)
   are effectively the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[REG_RSP]

# elif defined __i386__ /* 32 bit registers */

/* See glibc/sysdeps/unix/sysv/linux/x86/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/i386/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/x86/sys/ucontext.h
   and the 'struct sigcontext_ia32' defined in <asm/sigcontext32.h>
   are effectively the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[REG_ESP]
                     /* same value as ((ucontext_t *) ucp)->uc_mcontext.gregs[REG_UESP] */

# elif defined __ia64__

/* See glibc/sysdeps/unix/sysv/linux/ia64/sys/ucontext.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/ia64/sys/ucontext.h
   and the 'struct sigcontext' defined in
   glibc/sysdeps/unix/sysv/linux/ia64/bits/sigcontext.h
   (see also <asm/sigcontext.h>)
   are actually the same.  */

/* IA-64 has two stack pointers, one that grows down, called $r12, and one
   that grows up, called $bsp/$bspstore.  */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.sc_gr[12]

/* It would be better to access $bspstore instead of $bsp but I don't know
   where to find it in 'struct sigcontext'.  Anyway, it doesn't matter
   because $bsp and $bspstore never differ by more than ca. 1 KB.  */
#  define SIGSEGV_FAULT_BSP_POINTER  ((ucontext_t *) ucp)->uc_mcontext.sc_ar_bsp

# elif defined __loongarch__

/* See <sys/ucontext.h>.
   Note that the 'mcontext_t' defined in <sys/ucontext.h>
   and the 'struct sigcontext' defined in <bits/sigcontext.h>
   (see also <asm/sigcontext.h>) are effectively the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.__gregs[3]

# elif defined __m68k__

/* See glibc/sysdeps/unix/sysv/linux/m68k/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/m68k/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/m68k/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are quite different types.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[R_SP]

# elif defined __mips__ || defined __mipsn32__ || defined __mips64__

/* See glibc/sysdeps/unix/sysv/linux/mips/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/mips/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/mips/sys/ucontext.h
   and the 'struct sigcontext' defined in
   glibc/sysdeps/unix/sysv/linux/mips/bits/sigcontext.h
   (see also <asm/sigcontext.h>)
   are effectively the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[29]

# elif defined __nds32__

/* See glibc/sysdeps/unix/sysv/linux/nds32/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/nds32/sigcontextinfo.h.
   Both are found in <https://patches-gcc.linaro.org/cover/4409/> part 08/11
   <https://sourceware.org/ml/libc-alpha/2018-05/msg00125.html>.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.nds32_sp

# elif defined __powerpc__ || defined __powerpc64__ || defined __powerpc64_elfv2__

/* See glibc/sysdeps/unix/sysv/linux/powerpc/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/powerpc/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/powerpc/sys/ucontext.h,
   the 'struct sigcontext' defined in <asm/sigcontext.h>,
   and the 'struct pt_regs' defined in <asm/ptrace.h>
   are quite different types.  */

#  if defined __powerpc64__ || defined __powerpc64_elfv2__ /* 64-bit */
#   define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gp_regs[1]
#  else /* 32-bit */
#   if MUSL_LIBC
/* musl libc has a different structure of ucontext_t in
   musl/arch/powerpc/bits/signal.h.  */
/* The glibc comments say:
     "Different versions of the kernel have stored the registers on signal
      delivery at different offsets from the ucontext struct.  Programs should
      thus use the uc_mcontext.uc_regs pointer to find where the registers are
      actually stored."  */
#    if 0
#     define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[1]
#    else
#     define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_regs->gregs[1]
#    endif
#   else
/* Assume the structure of ucontext_t in
   glibc/sysdeps/unix/sysv/linux/powerpc/sys/ucontext.h.  */
/* Because of the union, both definitions should be equivalent.  */
#    if 0
#     define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.regs->gpr[1]
#    else
#     define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.uc_regs->gregs[1]
#    endif
#   endif
#  endif

# elif defined __riscv32__ || __riscv64__

/* See glibc/sysdeps/unix/sysv/linux/riscv/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/riscv/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/riscv/sys/ucontext.h
   and the 'struct sigcontext' defined in
   glibc/sysdeps/unix/sysv/linux/riscv/bits/sigcontext.h
   start with the same block of 32 general-purpose registers.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.__gregs[REG_SP]

# elif defined __s390__ || defined __s390x__

/* See glibc/sysdeps/unix/sysv/linux/s390/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/s390/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/s390/sys/ucontext.h
   and the '_sigregs' type, indirect part of 'struct sigcontext', defined
   in <asm/sigcontext.h>, are effectively the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[15]

# elif defined __sh__

/* See glibc/sysdeps/unix/sysv/linux/sh/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/sh/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/sh/sys/ucontext.h
   and the 'struct sigcontext' defined in <asm/sigcontext.h>
   are effectively the same.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[15]

# elif defined __sparc__ || defined __sparc64__

/* See glibc/sysdeps/unix/sysv/linux/sparc/sys/ucontext.h
   and the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/sparc/{sparc32,sparc64}/sigcontextinfo.h.
   Note that the 'mcontext_t' defined in
   glibc/sysdeps/unix/sysv/linux/sparc/sys/ucontext.h
   and the 'struct sigcontext' defined in
   glibc/sysdeps/unix/sysv/linux/sparc/bits/sigcontext.h
   (see also <asm/sigcontext.h>)
   are quite different types.  */

#  if defined __sparc64__/* 64-bit */
/* From linux-4.8.1/arch/sparc/kernel/signal_64.c, function setup_rt_frame, we
   see that ucp is not an 'ucontext_t *' but rather a 'struct sigcontext *'
   that happens to have the same value as sip (which is possible because a
   'struct sigcontext' starts with 128 bytes room for the siginfo_t).  */
#   define SIGSEGV_FAULT_STACKPOINTER  (((struct sigcontext *) ucp)->sigc_regs.u_regs[14] + 2047)
#  else /* 32-bit */
/* From linux-4.8.1/arch/sparc/kernel/signal_32.c, function setup_rt_frame,
   and linux-4.8.1/arch/sparc/kernel/signal32.c, function setup_rt_frame32, we
   see that ucp is a 'struct pt_regs *' or 'struct pt_regs32 *', respectively.
   In userland, this is a 'struct sigcontext *'.  */
#   define SIGSEGV_FAULT_STACKPOINTER  ((struct sigcontext *) ucp)->si_regs.u_regs[14]
#  endif

/* The sip->si_addr field is correct for a normal fault, but unusable in case
   of a stack overflow. What I observe (when running
   tests/test-sigsegv-catch-stackoverflow1, with a printf right at the beginning
   of sigsegv_handler) is that sip->si_addr is near 0:
     - in 64-bit mode: sip->si_addr = 0x000000000000030F, and gdb shows me that
       the fault occurs in an instruction 'stx %o3,[%fp+0x30f]' and %fp is 0.
       In fact, all registers %l0..%l7 and %i0..%i7 are 0.
     - in 32-bit mode: sip->si_addr = 0xFFFFFA64, and gdb shows me that
       the fault occurs in an instruction 'st %g2,[%fp-1436]' and %fp is 0.
       In fact, all registers %l0..%l7 and %i0..%i7 are 0.
   Apparently when the stack overflow occurred, some trap has tried to move the
   contents of the registers %l0..%l7 and %i0..%i7 (a "window" in SPARC
   terminology) to the stack, did not succeed in doing this, replaced all these
   register values with 0, and resumed execution at the fault location. This
   time, due to %fp = 0, a different fault was triggered. Now it is impossible
   to determine the real (previous) fault address because, even if know the
   faulting instruction, the previous register values have been lost.  */
#  define BOGUS_FAULT_ADDRESS_UPON_STACK_OVERFLOW

# else

/* When adding support for other CPUs here:  */

/* For SIGSEGV_FAULT_HANDLER_ARGLIST, see the definition of SIGCONTEXT in
   glibc/sysdeps/unix/sysv/linux/<cpu>/sigcontextinfo.h.  */

/* For SIGSEGV_FAULT_STACKPOINTER, see the definition of GET_STACK in
   glibc/sysdeps/unix/sysv/linux/<cpu>/sigcontextinfo.h.  */

# endif

#endif

#if defined __ANDROID__ /* Android */
/* A platform that supports the POSIX:2008 (XPG 7) way, without
   'struct sigcontext' nor 'ucontext_t'.  */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *context
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  context
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

#endif

#if defined __gnu_hurd__ /* Hurd */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, long code, struct sigcontext *scp
# define SIGSEGV_FAULT_ADDRESS  (unsigned long) code
# define SIGSEGV_FAULT_CONTEXT  scp

# if defined __x86_64__
/* 64 bit registers */

/* scp points to a 'struct sigcontext' (defined in
   glibc/sysdeps/mach/hurd/x86_64/bits/sigcontext.h).
   The registers, at the moment the signal occurred, get pushed on the kernel
   stack through gnumach/x86_64/locore.S:alltraps. They are denoted by a
   'struct i386_saved_state' (defined in gnumach/i386/i386/thread.h).
   Upon invocation of the Mach interface function thread_get_state
   <https://www.gnu.org/software/hurd/gnumach-doc/Thread-Execution.html>
   (= __thread_get_state in glibc), defined in gnumach/kern/thread.c,
   the function thread_getstatus, defined in gnumach/i386/i386/pcb.c, copies the
   register values in a different arrangement into a 'struct i386_thread_state',
   defined in gnumach/i386/include/mach/i386/thread_status.h. (Different
   arrangement: trapno, err get dropped; different order of r8...r15; also rsp
   gets set to 0.)
   This 'struct i386_thread_state' is actually the 'basic' part of a
   'struct machine_thread_all_state', defined in
   glibc/sysdeps/mach/x86/thread_state.h.
   From there, the function _hurd_setup_sighandler, defined in
   glibc/sysdeps/mach/hurd/x86/trampoline.c,
   1. sets rsp to the same value as ursp,
   2. copies the 'struct i386_thread_state' into the appropriate part of a
      'struct sigcontext', defined in
      glibc/sysdeps/mach/hurd/x86_64/bits/sigcontext.h.  */
/* Both sc_rsp and sc_ursp have the same value.
   It appears more reliable to use sc_ursp because sc_rsp is marked as
   "not used".  */
#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_ursp

# elif defined __i386__
/* 32 bit registers */

/* scp points to a 'struct sigcontext' (defined in
   glibc/sysdeps/mach/hurd/i386/bits/sigcontext.h).
   The registers, at the moment the signal occurred, get pushed on the kernel
   stack through gnumach/i386/i386/locore.S:alltraps. They are denoted by a
   'struct i386_saved_state' (defined in gnumach/i386/i386/thread.h).
   Upon invocation of the Mach interface function thread_get_state
   <https://www.gnu.org/software/hurd/gnumach-doc/Thread-Execution.html>
   (= __thread_get_state in glibc), defined in gnumach/kern/thread.c,
   the function thread_getstatus, defined in gnumach/i386/i386/pcb.c, copies the
   register values in a different arrangement into a 'struct i386_thread_state',
   defined in gnumach/i386/include/mach/i386/thread_status.h. (Different
   arrangement: trapno, err get dropped; also esp gets set to 0.)
   This 'struct i386_thread_state' is actually the 'basic' part of a
   'struct machine_thread_all_state', defined in
   glibc/sysdeps/mach/x86/thread_state.h.
   From there, the function _hurd_setup_sighandler, defined in
   glibc/sysdeps/mach/hurd/x86/trampoline.c,
   1. sets esp to the same value as uesp,
   2. copies the 'struct i386_thread_state' into the appropriate part of a
      'struct sigcontext', defined in
      glibc/sysdeps/mach/hurd/i386/bits/sigcontext.h.  */
/* Both sc_esp and sc_uesp have the same value.
   It appears more reliable to use sc_uesp because sc_esp is marked as
   "not used".  */
#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_uesp

# endif

#endif

#if defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ /* GNU/kFreeBSD, FreeBSD */

# if defined __arm__ || defined __armhf__ || (defined __arm64__ || defined __aarch64__)

#  define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
#  define SIGSEGV_FAULT_ADDRESS  sip->si_addr
#  define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)

#  if defined __arm64__ || defined __aarch64__ /* 64-bit */

/* See sys/arm64/include/ucontext.h.  */

#   if defined __CHERI_PURE_CAPABILITY__
#    define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.mc_capregs.cap_sp
#   else
#    define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.mc_gpregs.gp_sp
#   endif

#  elif defined __arm__ || defined __armhf__ /* 32-bit */

/* See sys/arm/include/ucontext.h.  */

#   define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.__gregs[_REG_SP]

#  endif

# else

/* On FreeBSD 12, both of these approaches work.  On FreeBSD derivatives, the
   first one has more chances to work.  */
#  if 1
/* Use signal handlers without SA_SIGINFO.  */

#   define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, int code, struct sigcontext *scp, void *addr
#   define SIGSEGV_FAULT_ADDRESS  addr
#   define SIGSEGV_FAULT_CONTEXT  scp

/* See sys/x86/include/signal.h.  */

#   if defined __x86_64__
/* 64 bit registers */

#    define SIGSEGV_FAULT_STACKPOINTER  scp->sc_rsp

#   elif defined __i386__
/* 32 bit registers */

#    define SIGSEGV_FAULT_STACKPOINTER  scp->sc_esp

#   endif

#  else
/* Use signal handlers with SA_SIGINFO.  */

#   define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *scp
#   define SIGSEGV_FAULT_ADDRESS  sip->si_addr
#   define SIGSEGV_FAULT_CONTEXT  ((struct sigcontext *) scp)
#   define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

/* See sys/x86/include/signal.h.  */

#   if defined __x86_64__
/* 64 bit registers */

#    define SIGSEGV_FAULT_STACKPOINTER  ((struct sigcontext *) scp)->sc_rsp

#   elif defined __i386__
/* 32 bit registers */

#    define SIGSEGV_FAULT_STACKPOINTER  ((struct sigcontext *) scp)->sc_esp

#   endif

#  endif

# endif

#endif

#if defined __NetBSD__ /* NetBSD */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

/* _UC_MACHINE_SP is a platform independent macro.
   Defined in <machine/mcontext.h>, see
     https://cvsweb.netbsd.org/bsdweb.cgi/src/sys/arch/$arch/include/mcontext.h
   Supported on alpha, amd64, i386, ia64, m68k, mips, powerpc, sparc since
   NetBSD 2.0.
   On i386, _UC_MACHINE_SP is the same as ->uc_mcontext.__gregs[_REG_UESP],
   and apparently the same value as       ->uc_mcontext.__gregs[_REG_ESP]. */
# ifdef _UC_MACHINE_SP
#  define SIGSEGV_FAULT_STACKPOINTER  _UC_MACHINE_SP ((ucontext_t *) ucp)
# endif

#endif

#if defined __OpenBSD__ /* OpenBSD */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, struct sigcontext *scp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  scp
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

# if defined __alpha__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/alpha/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_regs[30]

# elif defined __arm__ || defined __armhf__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/arm/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_usr_sp

# elif defined __hppa__ || defined __hppa64__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/hppa/include/signal.h
   and
   openbsd-src/sys/arch/hppa64/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_regs[30]

# elif defined __x86_64__
/* 64 bit registers */

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/amd64/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_rsp

# elif defined __i386__
/* 32 bit registers */

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/i386/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_esp

# elif defined __m68k__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/m68k/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_sp

# elif defined __m88k__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/m88k/include/signal.h
   and the definition of 'struct reg' in
   openbsd-src/sys/arch/m88k/include/reg.h.  */

#  if OpenBSD >= 201211 /* OpenBSD version >= 5.2 */
#   define SIGSEGV_FAULT_STACKPOINTER  scp->sc_regs[31]
#  else
#   define SIGSEGV_FAULT_STACKPOINTER  scp->sc_regs.r[31]
#  endif

# elif defined __mips__ || defined __mipsn32__ || defined __mips64__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/mips64/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_regs[29]

# elif defined __powerpc64__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/powerpc64/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_sp

# elif defined __powerpc__

/* See the definition of 'struct sigcontext' and 'struct trapframe' in
   openbsd-src/sys/arch/powerpc/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_frame.fixreg[1]

# elif defined __sh__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/sh/include/signal.h
   and the definition of 'struct reg' in
   openbsd-src/sys/arch/sh/include/reg.h.  */

#  if OpenBSD >= 201211 /* OpenBSD version >= 5.2 */
#   define SIGSEGV_FAULT_STACKPOINTER  scp->sc_reg[20-15]
#  else
#   define SIGSEGV_FAULT_STACKPOINTER  scp->sc_reg.r_r15
#  endif

# elif defined __sparc__ || defined __sparc64__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/sparc/include/signal.h
   and
   openbsd-src/sys/arch/sparc64/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_sp

# elif defined __vax__

/* See the definition of 'struct sigcontext' in
   openbsd-src/sys/arch/vax/include/signal.h.  */

#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_sp

# endif

#endif

#if (defined __APPLE__ && defined __MACH__) /* macOS */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

# if defined __x86_64__

/* See the definitions of
     - 'ucontext_t' and 'struct __darwin_ucontext' in <sys/_types/_ucontext.h>,
     - 'struct __darwin_mcontext64' in <i386/_mcontext.h>, and
     - 'struct __darwin_x86_thread_state64' in <mach/i386/_structs.h>.  */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext->__ss.__rsp

# elif defined __i386__

/* See the definitions of
     - 'ucontext_t' and 'struct __darwin_ucontext' in <sys/_types/_ucontext.h>,
     - 'struct __darwin_mcontext32' in <i386/_mcontext.h>, and
     - 'struct __darwin_i386_thread_state' in <mach/i386/_structs.h>.  */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext->__ss.__esp

# elif defined __arm64__

/* See the definitions of
     - 'ucontext_t' and 'struct __darwin_ucontext' in <sys/_types/_ucontext.h>,
     - 'struct __darwin_mcontext64' in <arm/_mcontext.h>, and
     - 'struct __darwin_arm_thread_state64' in <mach/arm/_structs.h>.  */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext->__ss.__sp

# elif defined __powerpc__

/* See the definitions of
     - 'ucontext_t' and 'struct __darwin_ucontext' in <sys/_structs.h>,
     - 'struct __darwin_mcontext' in <ppc/_structs.h>, and
     - 'struct __darwin_ppc_thread_state' in <mach/ppc/_structs.h>.  */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext->__ss.__r1

# endif

#endif

#if defined _AIX /* AIX */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

# if defined __powerpc__ || defined __powerpc64__
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.jmp_context.gpr[1]
# endif

#endif

#if defined __sgi /* IRIX */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, int code, struct sigcontext *scp
# define SIGSEGV_FAULT_ADDRESS  (unsigned long) scp->sc_badvaddr
# define SIGSEGV_FAULT_CONTEXT  scp

# if defined __mips__ || defined __mipsn32__ || defined __mips64__
#  define SIGSEGV_FAULT_STACKPOINTER  scp->sc_regs[29]
# endif

#endif

#if defined __sun /* Solaris */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

# if defined __x86_64__
/* 64 bit registers */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[REG_RSP]

# elif defined __i386__
/* 32 bit registers */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[ESP]

# elif defined __sparc__ || defined __sparc64__

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.gregs[REG_O6]

#  if SOLARIS11

/* On Solaris 11.3/SPARC, both in 32-bit and 64-bit mode, when catching
   stack overflow, the fault address is correct the first time, but is zero
   or near zero the second time.
   'truss tests/test-sigsegv-catch-stackoverflow1' shows it:

   In 32-bit mode:

    Incurred fault #6, FLTBOUNDS  %pc = 0x000116E8
      siginfo: SIGSEGV SEGV_MAPERR addr=0xFFB00000
    Received signal #11, SIGSEGV [caught]
      siginfo: SIGSEGV SEGV_MAPERR addr=0xFFB00000
   then
    Incurred fault #6, FLTBOUNDS  %pc = 0x000116E8
      siginfo: SIGSEGV SEGV_MAPERR addr=0x00000008
    Received signal #11, SIGSEGV [caught]
      siginfo: SIGSEGV SEGV_MAPERR addr=0x00000008

   In 64-bit mode:

    Incurred fault #6, FLTBOUNDS  %pc = 0x100001C58
      siginfo: SIGSEGV SEGV_MAPERR addr=0xFFFFFFFF7FF00000
    Received signal #11, SIGSEGV [caught]
      siginfo: SIGSEGV SEGV_MAPERR addr=0xFFFFFFFF7FF00000
   then
    Incurred fault #6, FLTBOUNDS  %pc = 0x100001C58
      siginfo: SIGSEGV SEGV_MAPERR addr=0x00000000
    Received signal #11, SIGSEGV [caught]
      siginfo: SIGSEGV SEGV_MAPERR addr=0x00000000
 */
#   define BOGUS_FAULT_ADDRESS_UPON_STACK_OVERFLOW

#  endif

# endif

#endif

#if defined __CYGWIN__ /* Cygwin */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

/* See the definition of 'ucontext_t' in <sys/ucontext.h> and
   of 'struct __mcontext' in <cygwin/signal.h>.  */
# if defined __x86_64__
/* 64 bit registers */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.rsp
# elif defined __i386__
/* 32 bit registers */
#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.esp
# endif

#endif

#if defined __HAIKU__ /* Haiku */

# define SIGSEGV_FAULT_HANDLER_ARGLIST  int sig, siginfo_t *sip, void *ucp
# define SIGSEGV_FAULT_ADDRESS  sip->si_addr
# define SIGSEGV_FAULT_CONTEXT  ((ucontext_t *) ucp)
# define SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO

# if defined __x86_64__
/* 64 bit registers */

/* See the definition of 'ucontext_t' in <signal.h> and
   of 'struct vregs' in <arch/x86_64/signal.h>.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.rsp

# elif defined __i386__
/* 32 bit registers */

/* See the definition of 'ucontext_t' in <signal.h> and
   of 'struct vregs' in <arch/x86/signal.h>.  */

#  define SIGSEGV_FAULT_STACKPOINTER  ((ucontext_t *) ucp)->uc_mcontext.esp

# endif

#endif

/* ========================================================================== */

/* List of signals that are sent when an invalid virtual memory address
   is accessed, or when the stack overflows.  */
#if defined __gnu_hurd__ \
    || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
    || defined __NetBSD__ || defined __OpenBSD__ \
    || (defined __APPLE__ && defined __MACH__)
# if defined __CHERI__
#  define SIGSEGV_FOR_ALL_SIGNALS(var,body) \
     { int var;                             \
       var = SIGSEGV; { body }              \
       var = SIGBUS; { body }               \
       var = SIGPROT; { body }              \
     }
# else
#  define SIGSEGV_FOR_ALL_SIGNALS(var,body) \
     { int var;                             \
       var = SIGSEGV; { body }              \
       var = SIGBUS; { body }               \
     }
# endif
#else
# define SIGSEGV_FOR_ALL_SIGNALS(var,body) \
    { int var; var = SIGSEGV; { body } }
#endif

/* ========================================================================== */

/* Determine the virtual memory area of a given address.  */
#include "stackvma.h"

/* ========================================================================== */

/* On the average Unix platform, we define

   HAVE_SIGSEGV_RECOVERY
       if there is a fault-*.h include file which defines
       SIGSEGV_FAULT_HANDLER_ARGLIST and SIGSEGV_FAULT_ADDRESS.

   HAVE_STACK_OVERFLOW_RECOVERY
       if HAVE_SIGALTSTACK is set and
       at least two of the following are true:
       A) There is a fault-*.h include file which defines
          SIGSEGV_FAULT_HANDLER_ARGLIST and SIGSEGV_FAULT_ADDRESS.
       B) There is a fault-*.h include file which defines
          SIGSEGV_FAULT_HANDLER_ARGLIST and SIGSEGV_FAULT_STACKPOINTER.
       C) There is a stackvma-*.c, other than stackvma-none.c, which
          defines sigsegv_get_vma.

   Why? Obviously, to catch stack overflow, we need an alternate signal
   stack; this requires kernel support. But we also need to distinguish
   (with a reasonable confidence) a stack overflow from a regular SIGSEGV.
   If we have A) and B), we use the
     Heuristic AB: If the fault address is near the stack pointer, it's a
     stack overflow.
   If we have A) and C), we use the
     Heuristic AC: If the fault address is near and beyond the bottom of
     the stack's virtual memory area, it's a stack overflow.
   If we have B) and C), we use the
     Heuristic BC: If the stack pointer is near the bottom of the stack's
     virtual memory area, it's a stack overflow.
     This heuristic comes in two flavours: On OSes which let the stack's
     VMA grow continuously, we determine the bottom by use of getrlimit().
     On OSes which preallocate the stack's VMA with its maximum size
     (like BeOS), we use the stack's VMA directly.
 */

#if HAVE_SIGSEGV_RECOVERY \
     && !(defined SIGSEGV_FAULT_HANDLER_ARGLIST && defined SIGSEGV_FAULT_ADDRESS)
# error "You need to define SIGSEGV_FAULT_HANDLER_ARGLIST and SIGSEGV_FAULT_ADDRESS before you can define HAVE_SIGSEGV_RECOVERY."
#endif
#if !HAVE_SIGSEGV_RECOVERY \
    && (defined SIGSEGV_FAULT_HANDLER_ARGLIST && defined SIGSEGV_FAULT_ADDRESS) \
    && !(defined __FreeBSD__ && (defined __sparc__ || defined __sparc64__))
# if __GNUC__ || (__clang_major__ >= 4)
#  warning "You can define HAVE_SIGSEGV_RECOVERY on this platform."
# else
#  error "You can define HAVE_SIGSEGV_RECOVERY on this platform."
# endif
#endif

#if HAVE_STACK_OVERFLOW_RECOVERY \
    && !(defined SIGSEGV_FAULT_ADDRESS + defined SIGSEGV_FAULT_STACKPOINTER + HAVE_STACKVMA >= 2)
# error "You need to define two of SIGSEGV_FAULT_ADDRESS, SIGSEGV_FAULT_STACKPOINTER, HAVE_STACKVMA, before you can define HAVE_STACK_OVERFLOW_RECOVERY."
#endif
#if !HAVE_STACK_OVERFLOW_RECOVERY \
    && (defined SIGSEGV_FAULT_ADDRESS + defined SIGSEGV_FAULT_STACKPOINTER + HAVE_STACKVMA >= 2) \
    && !(defined __FreeBSD__ && (defined __sparc__ || defined __sparc64__)) \
    && !(defined __NetBSD__ && (defined __sparc__ || defined __sparc64__))
# if __GNUC__ || (__clang_major__ >= 4)
#  warning "You can define HAVE_STACK_OVERFLOW_RECOVERY on this platform."
# else
#  error "You can define HAVE_STACK_OVERFLOW_RECOVERY on this platform."
# endif
#endif

/* ========================================================================== */

#if HAVE_STACK_OVERFLOW_RECOVERY

/* ======= Leaving a signal handler executing on the alternate stack ======= */

/* Platform dependent:
   Leaving a signal handler executing on the alternate stack.  */
static void sigsegv_reset_onstack_flag (void);

/* -------------------------- leave-sigaltstack.c -------------------------- */

# if defined __gnu_hurd__ \
     || defined __FreeBSD_kernel__ || defined __FreeBSD__ || defined __DragonFly__ \
     || defined __NetBSD__ || defined __OpenBSD__

static void
sigsegv_reset_onstack_flag (void)
{
  stack_t ss;

  if (sigaltstack (NULL, &ss) >= 0)
    {
      ss.ss_flags &= ~SS_ONSTACK;
      sigaltstack (&ss, NULL);
    }
}

/* --------------------------- leave-setcontext.c --------------------------- */

# elif defined __sgi || defined __sun /* IRIX, Solaris */

#  include <ucontext.h>

static void
sigsegv_reset_onstack_flag (void)
{
  ucontext_t uc;

  if (getcontext (&uc) >= 0)
    /* getcontext returns twice.  We are interested in the returned context
       only the first time, i.e. when the SS_ONSTACK bit is set.  */
    if (uc.uc_stack.ss_flags & SS_ONSTACK)
      {
        uc.uc_stack.ss_flags &= ~SS_ONSTACK;
        /* Note that setcontext() does not refill uc.  Therefore if
           setcontext() keeps SS_ONSTACK set in the kernel, either
           setcontext() will return -1 or getcontext() will return a
           second time, with the SS_ONSTACK bit being cleared.  */
        setcontext (&uc);
      }
}

/* ------------------------------ leave-nop.c ------------------------------ */

# else

static void
sigsegv_reset_onstack_flag (void)
{
  /* Nothing to do. sigaltstack() simply looks at the stack pointer,
     therefore SS_ONSTACK is not sticky.  */
}

# endif

/* ========================================================================== */

# if HAVE_STACKVMA

/* Address of the last byte belonging to the stack vma.  */
static uintptr_t stack_top = 0;

/* Needs to be called once only.  */
static void
remember_stack_top (void *some_variable_on_stack)
{
  struct vma_struct vma;

  if (sigsegv_get_vma ((uintptr_t) some_variable_on_stack, &vma) >= 0)
    stack_top = vma.end - 1;
}

# endif /* HAVE_STACKVMA */

static stackoverflow_handler_t stk_user_handler = (stackoverflow_handler_t)NULL;
static uintptr_t stk_extra_stack;
static size_t stk_extra_stack_size;

#endif /* HAVE_STACK_OVERFLOW_RECOVERY */

#if HAVE_SIGSEGV_RECOVERY

/* User's SIGSEGV handler.  */
static sigsegv_handler_t user_handler = (sigsegv_handler_t)NULL;

#endif /* HAVE_SIGSEGV_RECOVERY */


/* Our SIGSEGV handler, with OS dependent argument list.  */

#if HAVE_SIGSEGV_RECOVERY

static void
sigsegv_handler (SIGSEGV_FAULT_HANDLER_ARGLIST)
{
  void *address = (void *) (SIGSEGV_FAULT_ADDRESS);

# if HAVE_STACK_OVERFLOW_RECOVERY
#  if !(HAVE_STACKVMA || defined SIGSEGV_FAULT_STACKPOINTER)
#error "Insufficient heuristics for detecting a stack overflow.  Either define CFG_STACKVMA and HAVE_STACKVMA correctly, or define SIGSEGV_FAULT_STACKPOINTER correctly, or undefine HAVE_STACK_OVERFLOW_RECOVERY!"
#  endif

  /* Call user's handler.  */
  if (user_handler && (*user_handler) (address, 0))
    {
      /* Handler successful.  */
    }
  else
    {
      /* Handler declined responsibility.  */

      /* Did the user install a stack overflow handler?  */
      if (stk_user_handler)
        {
          /* See whether it was a stack overflow. If so, longjump away.  */
#  ifdef SIGSEGV_FAULT_STACKPOINTER
          uintptr_t old_sp = (uintptr_t) (SIGSEGV_FAULT_STACKPOINTER);
#   ifdef __ia64
          uintptr_t old_bsp = (uintptr_t) (SIGSEGV_FAULT_BSP_POINTER);
#   endif
#  endif

#  if HAVE_STACKVMA
          /* Were we able to determine the stack top?  */
          if (stack_top)
            {
              /* Determine stack bounds.  */
              int saved_errno;
              struct vma_struct vma;
              int ret;

              saved_errno = errno;
              ret = sigsegv_get_vma (stack_top, &vma);
              errno = saved_errno;
              if (ret >= 0)
                {
#   ifndef BOGUS_FAULT_ADDRESS_UPON_STACK_OVERFLOW
                  /* Heuristic AC: If the fault_address is nearer to the stack
                     segment's [start,end] than to the previous segment, we
                     consider it a stack overflow.
                     In the case of IA-64, we know that the previous segment
                     is the up-growing bsp segment, and either of the two
                     stacks can overflow.  */
                  uintptr_t addr = (uintptr_t) address;

#    ifdef __ia64
                  if (addr >= vma.prev_end && addr <= vma.end - 1)
#    else
#     if STACK_DIRECTION < 0
                  if (addr >= vma.start
                      ? (addr <= vma.end - 1)
                      : vma.is_near_this (addr, &vma))
#     else
                  if (addr <= vma.end - 1
                      ? (addr >= vma.start)
                      : vma.is_near_this (addr, &vma))
#     endif
#    endif
                    {
#   else /* BOGUS_FAULT_ADDRESS_UPON_STACK_OVERFLOW */
#    if HAVE_GETRLIMIT && defined RLIMIT_STACK
                  /* Heuristic BC: If the stack size has reached its maximal size,
                     and old_sp is near the low end, we consider it a stack
                     overflow.  */
                  struct rlimit rl;

                  saved_errno = errno;
                  ret = getrlimit (RLIMIT_STACK, &rl);
                  errno = saved_errno;
                  if (ret >= 0)
                    {
                      uintptr_t current_stack_size = vma.end - vma.start;
                      uintptr_t max_stack_size = rl.rlim_cur;
                      if (current_stack_size <= max_stack_size + 4096
                          && max_stack_size <= current_stack_size + 4096
#    else
                    {
                      if (1
#    endif
#    ifdef SIGSEGV_FAULT_STACKPOINTER
                          /* Heuristic BC: If we know old_sp, and it is neither
                             near the low end, nor in the alternate stack, then
                             it's probably not a stack overflow.  */
                          && ((old_sp >= stk_extra_stack
                               && old_sp <= stk_extra_stack + stk_extra_stack_size)
#     if STACK_DIRECTION < 0
                              || (old_sp <= vma.start + 4096
                                  && vma.start <= old_sp + 4096))
#     else
                              || (old_sp <= vma.end + 4096
                                  && vma.end <= old_sp + 4096))
#     endif
#    endif
                         )
#   endif /* BOGUS_FAULT_ADDRESS_UPON_STACK_OVERFLOW */
#  else /* !HAVE_STACKVMA */
          /* Heuristic AB: If the fault address is near the stack pointer,
             it's a stack overflow.  */
          uintptr_t addr = (uintptr_t) address;

          if ((addr <= old_sp + 4096 && old_sp <= addr + 4096)
#   ifdef __ia64
              || (addr <= old_bsp + 4096 && old_bsp <= addr + 4096)
#   endif
             )
            {
                {
                    {
#  endif /* !HAVE_STACKVMA */
                        {
#  ifdef SIGSEGV_FAULT_STACKPOINTER
                          int emergency =
                            (old_sp >= stk_extra_stack
                             && old_sp <= stk_extra_stack + stk_extra_stack_size);
                          stackoverflow_context_t context = (SIGSEGV_FAULT_CONTEXT);
#  else
                          int emergency = 0;
                          stackoverflow_context_t context = (void *) 0;
#  endif
                          /* Call user's handler.  */
                          (*stk_user_handler) (emergency, context);
                        }
                    }
                }
            }
        }
# endif /* HAVE_STACK_OVERFLOW_RECOVERY */

      if (user_handler && (*user_handler) (address, 1))
        {
          /* Handler successful.  */
        }
      else
        {
          /* Handler declined responsibility for real.  */

          /* Remove ourselves and dump core.  */
          SIGSEGV_FOR_ALL_SIGNALS (signo, signal (signo, SIG_DFL);)
        }

# if HAVE_STACK_OVERFLOW_RECOVERY
    }
# endif /* HAVE_STACK_OVERFLOW_RECOVERY */
}

#elif HAVE_STACK_OVERFLOW_RECOVERY

static void
# ifdef SIGSEGV_FAULT_STACKPOINTER
sigsegv_handler (SIGSEGV_FAULT_HANDLER_ARGLIST)
# else
sigsegv_handler (int sig)
# endif
{
# if !((HAVE_GETRLIMIT && defined RLIMIT_STACK) || defined SIGSEGV_FAULT_STACKPOINTER)
#  error "Insufficient heuristics for detecting a stack overflow.  Either define SIGSEGV_FAULT_STACKPOINTER correctly, or undefine HAVE_STACK_OVERFLOW_RECOVERY!"
# endif

  /* Did the user install a handler?  */
  if (stk_user_handler)
    {
      /* See whether it was a stack overflow.  If so, longjump away.  */
# ifdef SIGSEGV_FAULT_STACKPOINTER
      uintptr_t old_sp = (uintptr_t) (SIGSEGV_FAULT_STACKPOINTER);
# endif

      /* Were we able to determine the stack top?  */
      if (stack_top)
        {
          /* Determine stack bounds.  */
          int saved_errno;
          struct vma_struct vma;
          int ret;

          saved_errno = errno;
          ret = sigsegv_get_vma (stack_top, &vma);
          errno = saved_errno;
          if (ret >= 0)
            {
# if HAVE_GETRLIMIT && defined RLIMIT_STACK
              /* Heuristic BC: If the stack size has reached its maximal size,
                 and old_sp is near the low end, we consider it a stack
                 overflow.  */
              struct rlimit rl;

              saved_errno = errno;
              ret = getrlimit (RLIMIT_STACK, &rl);
              errno = saved_errno;
              if (ret >= 0)
                {
                  uintptr_t current_stack_size = vma.end - vma.start;
                  uintptr_t max_stack_size = rl.rlim_cur;
                  if (current_stack_size <= max_stack_size + 4096
                      && max_stack_size <= current_stack_size + 4096
# else
                {
                  if (1
# endif
# ifdef SIGSEGV_FAULT_STACKPOINTER
                      /* Heuristic BC: If we know old_sp, and it is neither
                         near the low end, nor in the alternate stack, then
                         it's probably not a stack overflow.  */
                      && ((old_sp >= stk_extra_stack
                           && old_sp <= stk_extra_stack + stk_extra_stack_size)
#  if STACK_DIRECTION < 0
                          || (old_sp <= vma.start + 4096
                              && vma.start <= old_sp + 4096))
#  else
                          || (old_sp <= vma.end + 4096
                              && vma.end <= old_sp + 4096))
#  endif
# endif
                     )
                    {
# ifdef SIGSEGV_FAULT_STACKPOINTER
                      int emergency =
                        (old_sp >= stk_extra_stack
                         && old_sp <= stk_extra_stack + stk_extra_stack_size);
                      stackoverflow_context_t context = (SIGSEGV_FAULT_CONTEXT);
# else
                      int emergency = 0;
                      stackoverflow_context_t context = (void *) 0;
# endif
                      /* Call user's handler.  */
                      (*stk_user_handler)(emergency,context);
                    }
                }
            }
        }
    }

  /* Remove ourselves and dump core.  */
  SIGSEGV_FOR_ALL_SIGNALS (signo, signal (signo, SIG_DFL);)
}

#endif


#if HAVE_SIGSEGV_RECOVERY || HAVE_STACK_OVERFLOW_RECOVERY

static void
install_for (int sig)
{
  struct sigaction action;

# ifdef SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO
  action.sa_sigaction = (void (*) (int, siginfo_t *, void *)) &sigsegv_handler;
# else
  action.sa_handler = (void (*) (int)) &sigsegv_handler;
# endif
  /* Block most signals while SIGSEGV is being handled.  */
  /* Signals SIGKILL, SIGSTOP cannot be blocked.  */
  /* Signals SIGCONT, SIGTSTP, SIGTTIN, SIGTTOU are not blocked because
     dealing with these signals seems dangerous.  */
  /* Signals SIGILL, SIGABRT, SIGFPE, SIGSEGV, SIGTRAP, SIGIOT, SIGEMT, SIGBUS,
     SIGSYS, SIGSTKFLT are not blocked because these are synchronous signals,
     which may require immediate intervention, otherwise the process may
     starve.  */
  sigemptyset (&action.sa_mask);
# ifdef SIGHUP
  sigaddset (&action.sa_mask,SIGHUP);
# endif
# ifdef SIGINT
  sigaddset (&action.sa_mask,SIGINT);
# endif
# ifdef SIGQUIT
  sigaddset (&action.sa_mask,SIGQUIT);
# endif
# ifdef SIGPIPE
  sigaddset (&action.sa_mask,SIGPIPE);
# endif
# ifdef SIGALRM
  sigaddset (&action.sa_mask,SIGALRM);
# endif
# ifdef SIGTERM
  sigaddset (&action.sa_mask,SIGTERM);
# endif
# ifdef SIGUSR1
  sigaddset (&action.sa_mask,SIGUSR1);
# endif
# ifdef SIGUSR2
  sigaddset (&action.sa_mask,SIGUSR2);
# endif
# ifdef SIGCHLD
  sigaddset (&action.sa_mask,SIGCHLD);
# endif
# ifdef SIGCLD
  sigaddset (&action.sa_mask,SIGCLD);
# endif
# ifdef SIGURG
  sigaddset (&action.sa_mask,SIGURG);
# endif
# ifdef SIGIO
  sigaddset (&action.sa_mask,SIGIO);
# endif
# ifdef SIGPOLL
  sigaddset (&action.sa_mask,SIGPOLL);
# endif
# ifdef SIGXCPU
  sigaddset (&action.sa_mask,SIGXCPU);
# endif
# ifdef SIGXFSZ
  sigaddset (&action.sa_mask,SIGXFSZ);
# endif
# ifdef SIGVTALRM
  sigaddset (&action.sa_mask,SIGVTALRM);
# endif
# ifdef SIGPROF
  sigaddset (&action.sa_mask,SIGPROF);
# endif
# ifdef SIGPWR
  sigaddset (&action.sa_mask,SIGPWR);
# endif
# ifdef SIGLOST
  sigaddset (&action.sa_mask,SIGLOST);
# endif
# ifdef SIGWINCH
  sigaddset (&action.sa_mask,SIGWINCH);
# endif
  /* Note that sigaction() implicitly adds sig itself to action.sa_mask.  */
  /* Ask the OS to provide a structure siginfo_t to the handler.  */
# ifdef SIGSEGV_FAULT_ADDRESS_FROM_SIGINFO
  action.sa_flags = SA_SIGINFO;
# else
  action.sa_flags = 0;
# endif
# if HAVE_STACK_OVERFLOW_RECOVERY && HAVE_SIGALTSTACK /* not BeOS */
  /* Work around Linux 2.2.5 bug: If SA_ONSTACK is specified but sigaltstack()
     has not been called, the kernel will busy loop, eating CPU time.  So
     avoid setting SA_ONSTACK until the user has requested stack overflow
     handling.  */
  if (stk_user_handler)
    action.sa_flags |= SA_ONSTACK;
# endif
  sigaction (sig, &action, (struct sigaction *) NULL);
}

#endif /* HAVE_SIGSEGV_RECOVERY || HAVE_STACK_OVERFLOW_RECOVERY */

int
sigsegv_install_handler (sigsegv_handler_t handler)
{
#if HAVE_SIGSEGV_RECOVERY
  user_handler = handler;

  SIGSEGV_FOR_ALL_SIGNALS (sig, install_for (sig);)

  return 0;
#else
  return -1;
#endif
}

void
sigsegv_deinstall_handler (void)
{
#if HAVE_SIGSEGV_RECOVERY
  user_handler = (sigsegv_handler_t)NULL;

# if HAVE_STACK_OVERFLOW_RECOVERY
  if (!stk_user_handler)
# endif
    {
      SIGSEGV_FOR_ALL_SIGNALS (sig, signal (sig, SIG_DFL);)
    }
#endif
}

int
sigsegv_leave_handler (void (*continuation) (void*, void*, void*),
                       void* cont_arg1, void* cont_arg2, void* cont_arg3)
{
#if HAVE_STACK_OVERFLOW_RECOVERY
  /*
   * Reset the system's knowledge that we are executing on the alternate
   * stack. If we didn't do that, siglongjmp would be needed instead of
   * longjmp to leave the signal handler.
   */
  sigsegv_reset_onstack_flag ();
#endif
  (*continuation) (cont_arg1, cont_arg2, cont_arg3);
  return 1;
}

int
stackoverflow_install_handler (stackoverflow_handler_t handler,
                               void *extra_stack, size_t extra_stack_size)
{
#if HAVE_STACK_OVERFLOW_RECOVERY
# if HAVE_STACKVMA
  if (!stack_top)
    {
      int dummy;
      remember_stack_top (&dummy);
      if (!stack_top)
        return -1;
    }
# endif

  stk_user_handler = handler;
  stk_extra_stack = (uintptr_t) extra_stack;
  stk_extra_stack_size = extra_stack_size;
  {
    stack_t ss;
# if SIGALTSTACK_SS_REVERSED
    ss.ss_sp = (char *) extra_stack + extra_stack_size - sizeof (void *);
    ss.ss_size = extra_stack_size - sizeof (void *);
# else
    ss.ss_sp = extra_stack;
    ss.ss_size = extra_stack_size;
# endif
    ss.ss_flags = 0; /* no SS_DISABLE */
    if (sigaltstack (&ss, (stack_t*)0) < 0)
      return -1;
  }

  /* Install the signal handlers with SA_ONSTACK.  */
  SIGSEGV_FOR_ALL_SIGNALS (sig, install_for (sig);)
  return 0;
#else
  return -1;
#endif
}

void
stackoverflow_deinstall_handler (void)
{
#if HAVE_STACK_OVERFLOW_RECOVERY
  stk_user_handler = (stackoverflow_handler_t) NULL;

# if HAVE_SIGSEGV_RECOVERY
  if (user_handler)
    {
      /* Reinstall the signal handlers without SA_ONSTACK, to avoid Linux
         bug.  */
      SIGSEGV_FOR_ALL_SIGNALS (sig, install_for (sig);)
    }
  else
# endif
    {
      SIGSEGV_FOR_ALL_SIGNALS (sig, signal (sig, SIG_DFL);)
    }

  {
    stack_t ss;
    ss.ss_flags = SS_DISABLE;
    if (sigaltstack (&ss, (stack_t *) 0) < 0)
      perror ("gnulib sigsegv (stackoverflow_deinstall_handler)");
  }
#endif
}
