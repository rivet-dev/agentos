/* Convert string to 'unsigned long', with error checking.

   Copyright (C) 1994-2025 Free Software Foundation, Inc.

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

#define __strtol strtoul
#define __strtol_t unsigned long int
#define __xstrtol xstrtoul
#define STRTOL_T_MINIMUM 0
#define STRTOL_T_MAXIMUM ULONG_MAX
#include "xstrtol.c"
