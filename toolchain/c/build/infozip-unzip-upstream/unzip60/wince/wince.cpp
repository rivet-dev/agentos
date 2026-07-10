/*
  Copyright (c) 1990-2003 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
//******************************************************************************
//
// File:        WINCE.CPP
//
// Description: This file implements all the Win32 APIs and C runtime functions
//              that the Info-ZIP code calls, but are not implemented natively
//              on Windows CE.
//
// Copyright:   All the source files for Pocket UnZip, except for components
//              written by the Info-ZIP group, are copyrighted 1997 by Steve P.
//              Miller.  The product "Pocket UnZip" itself is property of the
//              author and cannot be altered in any way without written consent
//              from Steve P. Miller.
//
// Disclaimer:  All project files are provided "as is" with no guarantee of
//              their correctness.  The authors are not liable for any outcome
//              that is the result of using this source.  The source for Pocket
//              UnZip has been placed in the public domain to help provide an
//              understanding of its implementation.  You are hereby granted
//              full permission to use this source in any way you wish, except
//              to alter Pocket UnZip itself.  For comments, suggestions, and
//              bug reports, please write to stevemil@pobox.com.
//
// Functions:   DebugOut
//              chmod
//              close
//              isatty
//              lseek
//              open
//              read
//              setmode
//              unlink
//              fflush
//              fgets
//              fileno
//              fopen
//              fprintf
//              fclose
//              putc
//              sprintf
//              _stricmp
//              _strupr
//              strrchr                 (non-_MBCS only)
//              isupper
//              stat
//              localtime
//
// Internal helper functions:
//              SafeGetTimeZoneInformation
//              GetTransitionTimeT
//              IsDST
//
//
// Date      Name          History
// --------  ------------  -----------------------------------------------------
// 02/01/97  Steve Miller  Created (Version 1.0 using Info-ZIP UnZip 5.30)
//
//******************************************************************************


extern "C" {
#define __WINCE_CPP
#define UNZIP_INTERNAL
#include "unzip.h"
}
#include <tchar.h> // Must be outside of extern "C" block


//******************************************************************************
//***** For all platforms - Our debug output function
//******************************************************************************

#ifdef DEBUG // RETAIL version is __inline and does not generate any code.

void DebugOut(LPCTSTR szFormat, ...) {
   TCHAR szBuffer[512] = TEXT("PUNZIP: ");

   va_list pArgs;
   va_start(pArgs, szFormat);
   _vsntprintf(szBuffer + 8, countof(szBuffer) - 10, szFormat, pArgs);
   va_end(pArgs);

   TCHAR *psz = szBuffer;
   while (psz = _tcschr(psz, TEXT('\n'))) {
      *psz = TEXT('|');
   }
   psz = szBuffer;
   while (psz = _tcschr(psz, TEXT('\r'))) {
      *psz = TEXT('|');
   }

   _tcscat(szBuffer, TEXT("\r\n"));

   OutputDebugString(szBuffer);
}

#endif // DEBUG


//******************************************************************************
//***** Windows CE Native
//******************************************************************************

#if defined(_WIN32_WCE)

//******************************************************************************
//***** Local Function Prototyopes
//******************************************************************************

static void SafeGetTimeZoneInformation(TIME_ZONE_INFORMATION *ptzi);
static time_t GetTransitionTimeT(TIME_ZONE_INFORMATION *ptzi,
                                 int year, BOOL fStartDST);
static BOOL IsDST(TIME_ZONE_INFORMATION *ptzi, time_t localTime);

//******************************************************************************
//***** IO.H functions
//******************************************************************************

//-- Called from fileio.c
int __cdecl chmod(const char *filename, int pmode) {
   // Called before unlink() to delete read-only files.

   DWORD dwAttribs = (pmode & _S_IWRITE) ? FILE_ATTRIBUTE_NORMAL : FILE_ATTRIBUTE_READONLY;

   TCHAR szPath[_MAX_PATH];
   MBSTOTSTR(szPath, filename, countof(szPath));
   return (SetFileAttributes(szPath, dwAttribs) ? 0 : -1);
}

//******************************************************************************
//-- Called from process.c
int __cdecl close(int handle) {
   return (CloseHandle((HANDLE)handle) ? 0 : -1);
}

//******************************************************************************
//-- Called from fileio.c
int __cdecl isatty(int handle) {
   // returns TRUE if handle is a terminal, console, printer, or serial port
   // called with 1 (stdout) and 2 (stderr)
   return 0;
}

//******************************************************************************
//-- Called from extract.c, fileio.c, process.c
long __cdecl lseek(int handle, long offset, int origin) {
   // SEEK_SET, SEEK_CUR, SEEK_END are equal to FILE_BEGIN, FILE_CURRENT, FILE_END
   return SetFilePointer((HANDLE)handle, offset, NULL, origin);
}

//******************************************************************************
//-- Called from fileio.c
int __cdecl open(const char *filename, int oflags, ...) {

   // The Info-Zip code currently only opens existing ZIP files for read
   // using open().

   DWORD dwAccess = 0;
   DWORD dwCreate = 0;

   switch (oflags & (_O_RDONLY | _O_WRONLY | _O_RDWR)) {
      case _O_RDONLY:
         dwAccess = GENERIC_READ;
         break;
      case _O_WRONLY:
         dwAccess = GENERIC_WRITE;
         break;
      case _O_RDWR:
         dwAccess = GENERIC_READ | GENERIC_WRITE;
         break;
   }
   switch (oflags & (O_CREAT | O_TRUNC)) {
      case _O_CREAT:
         dwCreate = OPEN_ALWAYS;
         break;
      case _O_CREAT | _O_TRUNC:
         dwCreate = CREATE_ALWAYS;
         break;
      default:
         dwCreate = OPEN_EXISTING;
         break;
   }

   TCHAR szPath[_MAX_PATH];
   MBSTOTSTR(szPath, filename, countof(szPath));
   HANDLE hFile = CreateFile(szPath, dwAccess,
                             FILE_SHARE_READ | FILE_SHARE_WRITE,
                             NULL, dwCreate, FILE_ATTRIBUTE_NORMAL, NULL);
   if ((hFile != INVALID_HANDLE_VALUE) && ((oflags & O_APPEND) == O_APPEND)) {
      SetFilePointer(hFile, 0, NULL, FILE_END);
   }
   return ((hFile == INVALID_HANDLE_VALUE) ? -1 : (int)hFile);
}

//******************************************************************************
//-- Called from extract.c, fileio.c, process.c
int __cdecl read(int handle, void *buffer, unsigned int count) {
   DWORD dwRead = 0;
   return (ReadFile((HANDLE)handle, buffer, count, &dwRead, NULL) ? dwRead : -1);
}

#if _WIN32_WCE < 211
//******************************************************************************
//-- Called from extract.c
int __cdecl setmode(int handle, int mode) {
   //TEXT/BINARY translation - currently always called with O_BINARY.
   return O_BINARY;
}
#endif

//******************************************************************************
//-- Called from fileio.c
int __cdecl unlink(const char *filename) {

   // Called to delete files before an extract overwrite.

   TCHAR szPath[_MAX_PATH];
   MBSTOTSTR(szPath, filename, countof(szPath));
   return (DeleteFile(szPath) ? 0: -1);
}

//******************************************************************************
//***** STDIO.H functions
//******************************************************************************
#if _WIN32_WCE < 211
// Old versions of Win CE prior to 2.11 do not support stdio library functions.
// We provide simplyfied replacements that are more or less copies of the
// UNIX style low level I/O API functions. Only unbuffered I/O in binary mode
// is supported.
//-- Called from fileio.c
int __cdecl fflush(FILE *stream) {
   return (FlushFileBuffers((HANDLE)stream) ? 0 : EOF);
}

//******************************************************************************
//-- Called from extract.c
char * __cdecl fgets(char *string, int n, FILE *stream) {
   // stream always equals "stdin" and fgets() should never be called.
   DebugOut(TEXT("WARNING: fgets(0x%08X, %d, %08X) called."), string, n, stream);
   return NULL;
}

//******************************************************************************
//-- Called from extract.c
int __cdecl fileno(FILE *stream) {
   return (int)stream;
}

//******************************************************************************
//-- Called from fileio.c
FILE * __cdecl fopen(const char *filename, const char *mode) {

   // fopen() is used to create all extracted files.

   DWORD dwAccess = 0;
   DWORD dwCreate = 0;
   BOOL  fAppend  = FALSE;

   if (strstr(mode, "r+")) {
      dwAccess = GENERIC_READ | GENERIC_WRITE;
      dwCreate = OPEN_EXISTING;
   } else if (strstr(mode, "w+")) {
      dwAccess = GENERIC_READ | GENERIC_WRITE;
      dwCreate = CREATE_ALWAYS;
   } else if (strstr(mode, "a+")) {
      dwAccess = GENERIC_READ | GENERIC_WRITE;
      dwCreate = OPEN_ALWAYS;
      fAppend = TRUE;
   } else if (strstr(mode, "r")) {
      dwAccess = GENERIC_READ;
      dwCreate = OPEN_EXISTING;
   } else if (strstr(mode, "w")) {
      dwAccess = GENERIC_WRITE;
      dwCreate = CREATE_ALWAYS;
   } else if (strstr(mode, "a")) {
      dwAccess = GENERIC_WRITE;
      dwCreate = OPEN_ALWAYS;
      fAppend  = TRUE;
   }

   TCHAR szPath[_MAX_PATH];
   MBSTOTSTR(szPath, filename, countof(szPath));
   HANDLE hFile = CreateFile(szPath, dwAccess,
                             FILE_SHARE_READ | FILE_SHARE_WRITE,
                             NULL, dwCreate, FILE_ATTRIBUTE_NORMAL, NULL);

   if (hFile == INVALID_HANDLE_VALUE) {
      return NULL;
   }

   if (fAppend) {
      SetFilePointer(hFile, 0, NULL, FILE_END);
   }

   return (FILE*)hFile;
}

//******************************************************************************
//-- Called from many sources when compiled for DEBUG
int __cdecl fprintf(FILE *stream, const char *format, ...) {

   // All standard output/error in Info-ZIP is handled through fprintf()
   if ((stream == stdout) || (stream == stderr)) {
      return 1;
   }

   // "stream" always equals "stderr" or "stdout" - log error if we see otherwise.
#ifdef UNICODE
   DebugOut(TEXT("WARNING: fprintf(0x%08X, \"%S\", ...) called."), stream, format);
#else
   DebugOut(TEXT("WARNING: fprintf(0x%08X, \"%s\", ...) called."), stream, format);
#endif
   return 0;
}

//******************************************************************************
//-- Called from fileio.c
int __cdecl fclose(FILE *stream) {
   return (CloseHandle((HANDLE)stream) ? 0 : EOF);
}

//******************************************************************************
//-- Called from fileio.c
int __cdecl putc(int c, FILE *stream) {
   DebugOut(TEXT("WARNING: putc(%d, 0x%08X) called."), c, stream);
   return 0;
}

//******************************************************************************
//-- Called from intrface.cpp, extract.c, fileio.c, list.c, process.c
int __cdecl sprintf(char *buffer, const char *format, ...) {

   WCHAR wszBuffer[512], wszFormat[512];

   MBSTOTSTR(wszFormat, format, countof(wszFormat));
   BOOL fPercent = FALSE;
   for (WCHAR *pwsz = wszFormat; *pwsz; pwsz++) {
      if (*pwsz == L'%') {
         fPercent = !fPercent;
      } else if (fPercent && (((*pwsz >= L'a') && (*pwsz <= L'z')) ||
                              ((*pwsz >= L'A') && (*pwsz <= L'Z'))))
      {
         if (*pwsz == L's') {
            *pwsz = L'S';
         } else if (*pwsz == L'S') {
            *pwsz = L's';
         }
         fPercent = FALSE;
      }
   }

   va_list pArgs;
   va_start(pArgs, format);
   _vsntprintf(wszBuffer, countof(wszBuffer), wszFormat, pArgs);
   va_end(pArgs);

   TSTRTOMBS(buffer, wszBuffer, countof(wszBuffer));

   return 0;
}
#endif /* _WIN32_WCE < 211 */

#ifndef POCKET_UNZIP
//******************************************************************************
//-- Called from unzip.c
void __cdecl perror(const char* errorText)
{
    OutputDebugString((LPCTSTR)errorText);
}
#endif // !POCKET_UNZIP

#ifdef USE_FWRITE
//******************************************************************************
//-- Called from fileio.c
void __cdecl setbuf(FILE *, char *)
{
    // We are using fwrite and the call to setbuf was to set the stream
    // unbuffered which is the default behaviour, we have nothing to do.
}
#endif // USE_FWRITE

//******************************************************************************
//***** STDLIB.H functions
//******************************************************************************

#ifdef _MBCS
int __cdecl mblen(const char *mbc, size_t mbszmax)
{
    // very simple cooked-down version of mblen() without any error handling
    // (Windows CE does not support multibyte charsets with a maximum char
    // length > 2 bytes)
    return (IsDBCSLeadByte((BYTE)*mbc) ? 2 : 1);
}
#endif /* _MBCS */

//******************************************************************************
//***** STRING.H functions
//******************************************************************************

//-- Called from winmain.cpp
int __cdecl _stricmp(const char *string1, const char *string2) {
   while (*string1 && ((*string1 | 0x20) == (*string2 | 0x20))) {
      string1++;
      string2++;
   }
   return (*string1 - *string2);
}

//******************************************************************************
//-- Called from intrface.cpp and winmain.cpp
char* __cdecl _strupr(char *string) {
   while (*string) {
      if ((*string >= 'a') && (*string <= 'z')) {
         *string -= 'a' - 'A';
      }
      string++;
   }
   return string;
}

//******************************************************************************
//-- Called from fileio.c ("open_input_file()")
char* __cdecl strerror(int errnum) {
   return "[errmsg not available]";
}

#ifndef _MBCS
//******************************************************************************
//-- Called from winmain.cpp
char* __cdecl strrchr(const char *string, int c) {

   // Walk to end of string.
   for (char *p = (char*)string; *p; p++) {
   }

   // Walk backwards looking for character.
   for (p--; p >= string; p--) {
      if ((int)*p == c) {
         return p;
      }
   }

   return NULL;
}
#endif /* !_MBCS */

//******************************************************************************
//***** CTYPE.H functions
//******************************************************************************

#if _WIN32_WCE < 300
int __cdecl isupper(int c) {
   return ((c >= 'A') && (c <= 'Z'));
}
#endif

//******************************************************************************
//***** STAT.H functions
//******************************************************************************

//-- Called fileio.c, process.c, intrface.cpp
int __cdecl stat(const char *path, struct stat *buffer) {

   // stat() is called on both the ZIP files and extracted files.

   // Clear our stat buffer to be safe.
   ZeroMemory(buffer, sizeof(struct stat));

   // Find the file/direcotry and fill in a WIN32_FIND_DATA structure.
   WIN32_FIND_DATA w32fd;
   ZeroMemory(&w32fd, sizeof(w32fd));

   TCHAR szPath[_MAX_PATH];
   MBSTOTSTR(szPath, path, countof(szPath));
   HANDLE hFind = FindFirstFile(szPath, &w32fd);

   // Bail out now if we could not find the file/directory.
   if (hFind == INVALID_HANDLE_VALUE) {
      return -1;
   }

   // Close the find.
   FindClose(hFind);

   // Mode flags that are currently used: S_IWRITE, S_IFMT, S_IFDIR, S_IEXEC
   if (w32fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
      buffer->st_mode = _S_IFDIR | _S_IREAD | _S_IEXEC;
   } else {
      buffer->st_mode = _S_IFREG | _S_IREAD;
   }
   if (!(w32fd.dwFileAttributes & FILE_ATTRIBUTE_READONLY)) {
      buffer->st_mode |= _S_IWRITE;
   }

   // Store the file size.
   buffer->st_size  = (_off_t)w32fd.nFileSizeLow;

   // Convert the modified FILETIME to a time_t and store it.
   DWORDLONG dwl = *(DWORDLONG*)&w32fd.ftLastWriteTime;
   buffer->st_mtime = (time_t)((dwl - (DWORDLONG)116444736000000000) / (DWORDLONG)10000000);

   return 0;
}

//******************************************************************************
//***** TIME.H functions
//******************************************************************************

// Evaluates to TRUE if 'y' is a leap year, otherwise FALSE
// #define IS_LEAP_YEAR(y) ((((y) % 4 == 0) && ((y) % 100 != 0)) || ((y) % 400 == 0))

// The macro below is a reduced version of the above macro.  It is valid for
// years between 1901 and 2099 which easily includes all years representable
// by the current implementation of time_t.
#define IS_LEAP_YEAR(y) (((y) & 3) == 0)

#define BASE_DOW          4                  // 1/1/1970 was a Thursday.
#define SECONDS_IN_A_DAY  (24L * 60L * 60L)  // Number of seconds in one day.

// Month to Year Day conversion array.
int M2YD[] = {
   0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365
};

// Month to Leap Year Day conversion array.
int M2LYD[] = {
   0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366
};

//******************************************************************************
//-- Called from list.c
struct tm * __cdecl localtime(const time_t *timer) {

   // Return value for localtime().  Source currently never references
   // more than one "tm" at a time, so the single return structure is ok.
   static struct tm g_tm;
   ZeroMemory(&g_tm, sizeof(g_tm));

   // Get our time zone information.
   TIME_ZONE_INFORMATION tzi;
   SafeGetTimeZoneInformation(&tzi);

   // Create a time_t that has been corrected for our time zone.
   time_t localTime = *timer - (tzi.Bias * 60L);

   // Decide if value is in Daylight Savings Time.
   if (g_tm.tm_isdst = (int)IsDST(&tzi, localTime)) {
      localTime -= tzi.DaylightBias * 60L; // usually 60 minutes
   } else {
      localTime -= tzi.StandardBias * 60L; // usually  0 minutes
   }

   // time_t   is a 32-bit value for the seconds since January 1, 1970
   // FILETIME is a 64-bit value for the number of 100-nanosecond intervals
   //          since January 1, 1601

   // Compute the FILETIME for the given local time.
   DWORDLONG dwl = ((DWORDLONG)116444736000000000 +
                   ((DWORDLONG)localTime * (DWORDLONG)10000000));
   FILETIME ft = *(FILETIME*)&dwl;

   // Convert the FILETIME to a SYSTEMTIME.
   SYSTEMTIME st;
   ZeroMemory(&st, sizeof(st));
   FileTimeToSystemTime(&ft, &st);

   // Finish filling in our "tm" structure.
   g_tm.tm_sec  = (int)st.wSecond;
   g_tm.tm_min  = (int)st.wMinute;
   g_tm.tm_hour = (int)st.wHour;
   g_tm.tm_mday = (int)st.wDay;
   g_tm.tm_mon  = (int)st.wMonth - 1;
   g_tm.tm_year = (int)st.wYear - 1900;

   return &g_tm;
}

//******************************************************************************
static void SafeGetTimeZoneInformation(TIME_ZONE_INFORMATION *ptzi)
{

   ZeroMemory(ptzi, sizeof(TIME_ZONE_INFORMATION));

   // Ask the OS for the standard/daylight rules for the current time zone.
   if ((GetTimeZoneInformation(ptzi) == 0xFFFFFFFF) ||
       (ptzi->StandardDate.wMonth > 12) || (ptzi->DaylightDate.wMonth > 12))
   {
      // If the OS fails us, we default to the United States' rules.
      ZeroMemory(ptzi, sizeof(TIME_ZONE_INFORMATION));
      ptzi->StandardDate.wMonth =  10;  // October
      ptzi->StandardDate.wDay   =   5;  // Last Sunday (DOW == 0)
      ptzi->StandardDate.wHour  =   2;  // At 2:00 AM
      ptzi->DaylightBias        = -60;  // One hour difference
      ptzi->DaylightDate.wMonth =   4;  // April
      ptzi->DaylightDate.wDay   =   1;  // First Sunday (DOW == 0)
      ptzi->DaylightDate.wHour  =   2;  // At 2:00 AM
   }
}

//******************************************************************************
static time_t GetTransitionTimeT(TIME_ZONE_INFORMATION *ptzi,
                                 int year, BOOL fStartDST)
{
   // We only handle years within the range that time_t supports.  We need to
   // handle the very end of 1969 since the local time could be up to 13 hours
   // into the previous year.  In this case, our code will actually return a
   // negative value, but it will be compared to another negative value and is
   // handled correctly.  The same goes for the 13 hours past a the max time_t
   // value of 0x7FFFFFFF (in the year 2038).  Again, these values are handled
   // correctly as well.

   if ((year < 1969) || (year > 2038)) {
      return (time_t)0;
   }

   SYSTEMTIME *pst = fStartDST ? &ptzi->DaylightDate : &ptzi->StandardDate;

   // WORD wYear          Year (0000 == 0)
   // WORD wMonth         Month (January == 1)
   // WORD wDayOfWeek     Day of week (Sunday == 0)
   // WORD wDay           Month day (1 - 31)
   // WORD wHour          Hour (0 - 23)
   // WORD wMinute        Minute (0 - 59)
   // WORD wSecond        Second (0 - 59)
   // WORD wMilliseconds  Milliseconds (0 - 999)

   // Compute the number of days since 1/1/1970 to the beginning of this year.
   long daysToYear = ((year - 1970) * 365) // Tally up previous years.
                   + ((year - 1969) >> 2); // Add few extra for the leap years.

   // Compute the number of days since the beginning of this year to the
   // beginning of the month.  We will add to this value to get the actual
   // year day.
   long yearDay = IS_LEAP_YEAR(year) ? M2LYD[pst->wMonth - 1] :
                                       M2YD [pst->wMonth - 1];

   // Check for day-in-month format.
   if (pst->wYear == 0) {

      // Compute the week day for the first day of the month (Sunday == 0).
      long monthDOW = (daysToYear + yearDay + BASE_DOW) % 7;

      // Add the day offset of the transition day to the year day.
      if (monthDOW < pst->wDayOfWeek) {
         yearDay += (pst->wDayOfWeek - monthDOW) + (pst->wDay - 1) * 7;
      } else {
         yearDay += (pst->wDayOfWeek - monthDOW) + pst->wDay * 7;
      }

      // It is possible that we overshot the month, especially if pst->wDay
      // is 5 (which means the last instance of the day in the month). Check
      // if the year-day has exceeded the month and adjust accordingly.
      if ((pst->wDay == 5) &&
          (yearDay >= (IS_LEAP_YEAR(year) ? M2LYD[pst->wMonth] :
                                            M2YD [pst->wMonth])))
      {
         yearDay -= 7;
      }

   // If not day-in-month format, then we assume an absolute date.
   } else {

      // Simply add the month day to the current year day.
      yearDay += pst->wDay - 1;
   }

   // Tally up all our days, hours, minutes, and seconds since 1970.
   long seconds = ((SECONDS_IN_A_DAY * (daysToYear + yearDay)) +
                   (3600L * (long)pst->wHour) +
                   (60L * (long)pst->wMinute) +
                   (long)pst->wSecond);

   // If we are checking for the end of DST, then we need to add the DST bias
   // since we are in DST when we chack this time stamp.
   if (!fStartDST) {
      seconds += ptzi->DaylightBias * 60L;
   }

   return (time_t)seconds;
}

//******************************************************************************
static BOOL IsDST(TIME_ZONE_INFORMATION *ptzi, time_t localTime) {

   // If either of the months is 0, then this usually means that the time zone
   // does not use DST.  Unfortunately, Windows CE since it has a bug where it
   // never really fills in these fields with the correct values, so it appears
   // like we are never in DST.  This is supposed to be fixed in future releases,
   // so hopefully this code will get some use then.
   if ((ptzi->StandardDate.wMonth == 0) || (ptzi->DaylightDate.wMonth == 0)) {
      return FALSE;
   }

   // time_t   is a 32-bit value for the seconds since January 1, 1970
   // FILETIME is a 64-bit value for the number of 100-nanosecond intervals
   //          since January 1, 1601

   // Compute the FILETIME for the given local time.
   DWORDLONG dwl = ((DWORDLONG)116444736000000000 +
                   ((DWORDLONG)localTime * (DWORDLONG)10000000));
   FILETIME ft = *(FILETIME*)&dwl;

   // Convert the FILETIME to a SYSTEMTIME.
   SYSTEMTIME st;
   ZeroMemory(&st, sizeof(st));
   FileTimeToSystemTime(&ft, &st);

   // Get our start and end daylight savings times.
   time_t timeStart = GetTransitionTimeT(ptzi, (int)st.wYear, TRUE);
   time_t timeEnd   = GetTransitionTimeT(ptzi, (int)st.wYear, FALSE);

   // Check what hemisphere we are in.
   if (timeStart < timeEnd) {

      // Northern hemisphere ordering.
      return ((localTime >= timeStart) && (localTime < timeEnd));

   } else if (timeStart > timeEnd) {

      // Southern hemisphere ordering.
      return ((localTime < timeEnd) || (localTime >= timeStart));
   }

   // If timeStart equals timeEnd then this time zone does not support DST.
   return FALSE;
}

#endif // _WIN32_WCE

//******************************************************************************
//***** Functions to supply timezone information from the Windows registry to
//***** Info-ZIP's private RTL "localtime() et al." replacements in timezone.c.
//******************************************************************************

//******************************************************************************
// Copied from win32.c
#ifdef W32_USE_IZ_TIMEZONE
#include "timezone.h"
#define SECSPERMIN      60
#define MINSPERHOUR     60
#define SECSPERHOUR     (SECSPERMIN * MINSPERHOUR)
static void conv_to_rule(LPSYSTEMTIME lpw32tm, struct rule * ZCONST ptrule);

static void conv_to_rule(LPSYSTEMTIME lpw32tm, struct rule * ZCONST ptrule)
{
    if (lpw32tm->wYear != 0) {
        ptrule->r_type = JULIAN_DAY;
        ptrule->r_day = ydays[lpw32tm->wMonth - 1] + lpw32tm->wDay;
    } else {
        ptrule->r_type = MONTH_NTH_DAY_OF_WEEK;
        ptrule->r_mon = lpw32tm->wMonth;
        ptrule->r_day = lpw32tm->wDayOfWeek;
        ptrule->r_week = lpw32tm->wDay;
    }
    ptrule->r_time = (long)lpw32tm->wHour * SECSPERHOUR +
                     (long)(lpw32tm->wMinute * SECSPERMIN) +
                     (long)lpw32tm->wSecond;
}

int GetPlatformLocalTimezone(register struct state * ZCONST sp,
        void (*fill_tzstate_from_rules)(struct state * ZCONST sp_res,
                                        ZCONST struct rule * ZCONST start,
                                        ZCONST struct rule * ZCONST end))
{
    TIME_ZONE_INFORMATION tzinfo;
    DWORD res;

    /* read current timezone settings from registry if TZ envvar missing */
    res = GetTimeZoneInformation(&tzinfo);
    if (res != TIME_ZONE_ID_INVALID)
    {
        struct rule startrule, stoprule;

        conv_to_rule(&(tzinfo.StandardDate), &stoprule);
        conv_to_rule(&(tzinfo.DaylightDate), &startrule);
        sp->timecnt = 0;
        sp->ttis[0].tt_abbrind = 0;
        if ((sp->charcnt =
             WideCharToMultiByte(CP_ACP, 0, tzinfo.StandardName, -1,
                                 sp->chars, sizeof(sp->chars), NULL, NULL))
            == 0)
            sp->chars[sp->charcnt++] = '\0';
        sp->ttis[1].tt_abbrind = sp->charcnt;
        sp->charcnt +=
            WideCharToMultiByte(CP_ACP, 0, tzinfo.DaylightName, -1,
                                sp->chars + sp->charcnt,
                                sizeof(sp->chars) - sp->charcnt, NULL, NULL);
        if ((sp->charcnt - sp->ttis[1].tt_abbrind) == 0)
            sp->chars[sp->charcnt++] = '\0';
        sp->ttis[0].tt_gmtoff = - (tzinfo.Bias + tzinfo.StandardBias)
                                * MINSPERHOUR;
        sp->ttis[1].tt_gmtoff = - (tzinfo.Bias + tzinfo.DaylightBias)
                                * MINSPERHOUR;
        sp->ttis[0].tt_isdst = 0;
        sp->ttis[1].tt_isdst = 1;
        sp->typecnt = (startrule.r_mon == 0 && stoprule.r_mon == 0) ? 1 : 2;

        if (sp->typecnt > 1)
            (*fill_tzstate_from_rules)(sp, &startrule, &stoprule);
        return TRUE;
    }
    return FALSE;
}
#endif /* W32_USE_IZ_TIMEZONE */
