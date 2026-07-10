/* Open a directory relative to another directory.

   Copyright 2006-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.

   Written by Jim Meyering and Paul Eggert.  */

#include <config.h>

#include <opendirat.h>

#include <errno.h>
#include <fcntl.h>
#include <unistd.h>

#ifdef GNULIB_OPENAT_SAFER
# include "fcntl--.h"
#endif

/* Relative to DIR_FD, open the directory DIR, passing EXTRA_FLAGS to
   the underlying openat call.  On success, store into *PNEW_FD the
   underlying file descriptor of the newly opened directory and return
   the directory stream.  On failure, return NULL and set errno.

   On success, *PNEW_FD is at least 3, so this is a "safer" function.  */

DIR *
opendirat (int dir_fd, char const *dir, int extra_flags, int *pnew_fd)
{
  int open_flags = (O_RDONLY | O_CLOEXEC | O_DIRECTORY | O_NOCTTY
                    | O_NONBLOCK | extra_flags);
  int new_fd = openat (dir_fd, dir, open_flags);

  if (new_fd < 0)
    return NULL;
  DIR *dirp = fdopendir (new_fd);
  if (dirp)
    *pnew_fd = new_fd;
  else
    {
      int fdopendir_errno = errno;
      close (new_fd);
      errno = fdopendir_errno;
    }
  return dirp;
}
