/* A more useful interface to strtoimax.

   Copyright (C) 2001-2025 Free Software Foundation, Inc.

   This file is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published
   by the Free Software Foundation, either version 3 of the License,
   or (at your option) any later version.

   This file is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

#define __strtol strtoimax
#define __strtol_t intmax_t
#define __xstrtol xstrtoimax
#define STRTOL_T_MINIMUM INTMAX_MIN
#define STRTOL_T_MAXIMUM INTMAX_MAX
#define XSTRTOL_INCLUDE_INTTYPES_H 1
#include "xstrtol.c"
