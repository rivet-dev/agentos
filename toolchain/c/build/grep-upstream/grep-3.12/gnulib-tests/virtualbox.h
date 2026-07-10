/* Determine whether the current system is running under VirtualBox/KVM.
   Copyright (C) 2021-2025 Free Software Foundation, Inc.

   This file is free software: you can redistribute it and/or modify
   it under the terms of the GNU Lesser General Public License as
   published by the Free Software Foundation; either version 2.1 of the
   License, or (at your option) any later version.

   This file is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible <bruno@clisp.org>, 2024.  */

#ifdef __linux__
# include <fcntl.h>
# include <string.h>
# include <unistd.h>
#endif

/* This function determines whether the current system is Linux and running
   under the VirtualBox emulator.  */
_GL_ATTRIBUTE_MAYBE_UNUSED static bool
is_running_under_virtualbox (void)
{
#ifdef __linux__
  /* On distributions with systemd, this could be done through
       test `systemd-detect-virt --vm` = oracle
     More generally, it can be done through
       test "`cat /sys/class/dmi/id/product_name`" = VirtualBox
     This is what we do here.  */
  char buf[4096];
  int fd = open ("/sys/class/dmi/id/product_name", O_RDONLY);
  if (fd >= 0)
    {
      int n = read (fd, buf, sizeof (buf));
      close (fd);
      if (n == 10 + 1 && memcmp (buf, "VirtualBox\n", 10 + 1) == 0)
        return true;
    }
#endif

  return false;
}

/* This function determines whether the current system is Linux and running
   under the VirtualBox emulator, with paravirtualization acceleration set to
   "Default" or "KVM".  */
static bool
is_running_under_virtualbox_kvm (void)
{
#ifdef __linux__
  if (is_running_under_virtualbox ())
    {
      /* As root, one can determine this paravirtualization mode through
           dmesg | grep -i kvm
         which produces output like this:
           [    0.000000] Hypervisor detected: KVM
           [    0.000000] kvm-clock: Using msrs 4b564d01 and 4b564d00
           [    0.000001] kvm-clock: using sched offset of 3736655524 cycles
           [    0.000004] clocksource: kvm-clock: mask: 0xffffffffffffffff max_cycles: 0x1cd42e4dffb, max_idle_ns: 881590591483 ns
           [    0.007355] Booting paravirtualized kernel on KVM
           [    0.213538] clocksource: Switched to clocksource kvm-clock
         So, we test whether the file
         /sys/devices/system/clocksource/clocksource0/available_clocksource
         contains the word 'kvm-clock'.  */
      char buf[4096 + 1];
      int fd = open ("/sys/devices/system/clocksource/clocksource0/available_clocksource", O_RDONLY);
      if (fd >= 0)
        {
          int n = read (fd, buf, sizeof (buf) - 1);
          close (fd);
          if (n > 0)
            {
              buf[n] = '\0';
              char *saveptr;
              char *word;
              for (word = strtok_r (buf, " \n", &saveptr);
                   word != NULL;
                   word = strtok_r (NULL, " \n", &saveptr))
                {
                  if (strcmp (word, "kvm-clock") == 0)
                    return true;
                }
            }
        }
    }
#endif

  return false;
}

/* This function returns the number of CPUs in the current system, assuming
   it is Linux.  */
static int
num_cpus (void)
{
#ifdef __linux__
  /* We could use sysconf (_SC_NPROCESSORS_CONF), which on glibc and musl libc
     is implemented through sched_getaffinity().  But there are some
     complications; see nproc.c.  It's simpler to parse /proc/cpuinfo.
     More precisely, it's sufficient to count the number of blank lines in
     /proc/cpuinfo.  */
  char buf[4096];
  int fd = open ("/proc/cpuinfo", O_RDONLY);
  if (fd >= 0)
    {
      unsigned int blank_lines = 0;
      bool last_char_was_newline = false;
      for (;;)
        {
          int n = read (fd, buf, sizeof (buf));
          if (n <= 0)
            break;
          int i;
          for (i = 0; i < n; i++)
            {
              if (last_char_was_newline && buf[i] == '\n')
                blank_lines++;
              last_char_was_newline = (buf[i] == '\n');
            }
        }
      close (fd);
      if (blank_lines > 0)
        return blank_lines;
    }
#endif

  return 1;
}
