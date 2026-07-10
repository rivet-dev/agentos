/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#ifndef __unzip_structs_h
#define __unzip_structs_h

#ifndef Far
#  define Far far
#endif

/* Porting definitions between Win 3.1x and Win32 */
#ifdef WIN32
#  define far
#  define _far
#  define __far
#  define near
#  define _near
#  define __near
#  ifndef FAR
#    define FAR
#  endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

#ifndef Z_UINT8_DEFINED
# if defined(__GNUC__)
   typedef unsigned long long    z_uint8;
#  define Z_UINT8_DEFINED
# elif (defined(_MSC_VER) && (_MSC_VER >= 1100))
   typedef unsigned __int64      z_uint8;
#  define Z_UINT8_DEFINED
# elif (defined(__WATCOMC__) && (__WATCOMC__ >= 1100))
   typedef unsigned __int64      z_uint8;
#  define Z_UINT8_DEFINED
# elif (defined(__IBMC__) && (__IBMC__ >= 350))
   typedef unsigned __int64      z_uint8;
#  define Z_UINT8_DEFINED
# elif (defined(__BORLANDC__) && (__BORLANDC__ >= 0x0500))
   typedef unsigned __int64      z_uint8;
#  define Z_UINT8_DEFINED
# elif (defined(__LCC__))
   typedef unsigned __int64      z_uint8;
#  define Z_UINT8_DEFINED
# endif
#endif

/* The following "function" types are jointly defined in both Zip and UnZip
 * DLLs.  They are guarded by the DEFINED_ONCE symbol to prevent multiple
 * declarations in applications that reference both the Zip and the UnZip DLL.
 */
#ifndef DEFINED_ONCE
#define DEFINED_ONCE

typedef int (WINAPI DLLPRNT) (LPSTR, unsigned long);
typedef int (WINAPI DLLPASSWORD) (LPSTR pwbuf, int bufsiz,
    LPCSTR promptmsg, LPCSTR entryname);
# ifdef Z_UINT8_DEFINED
typedef int (WINAPI DLLSERVICE) (LPCSTR entryname, z_uint8 uncomprsiz);
# else
typedef int (WINAPI DLLSERVICE) (LPCSTR entryname, unsigned long uncomprsiz);
# endif
typedef int (WINAPI DLLSERVICE_I32) (LPCSTR entryname,
    unsigned long ucsz_lo, unsigned long ucsz_hi);
#endif /* DEFINED_ONCE */

typedef void (WINAPI DLLSND) (void);
typedef int (WINAPI DLLREPLACE) (LPSTR efnam, unsigned efbufsiz);
#ifdef Z_UINT8_DEFINED
typedef void (WINAPI DLLMESSAGE) (z_uint8 ucsize, z_uint8 csize,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt);
#else
typedef void (WINAPI DLLMESSAGE) (unsigned long ucsize, unsigned long csize,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt);
#endif
typedef void (WINAPI DLLMESSAGE_I32) (unsigned long ucsiz_l,
    unsigned long ucsiz_h, unsigned long csiz_l, unsigned long csiz_h,
    unsigned cfactor,
    unsigned mo, unsigned dy, unsigned yr, unsigned hh, unsigned mm,
    char c, LPCSTR filename, LPCSTR methbuf, unsigned long crc, char fCrypt);

typedef struct {
  DLLPRNT *print;
  DLLSND *sound;
  DLLREPLACE *replace;
  DLLPASSWORD *password;
  DLLMESSAGE *SendApplicationMessage;
  DLLSERVICE *ServCallBk;
  DLLMESSAGE_I32 *SendApplicationMessage_i32;
  DLLSERVICE_I32 *ServCallBk_i32;
#ifdef Z_UINT8_DEFINED
  z_uint8 TotalSizeComp;
  z_uint8 TotalSize;
  z_uint8 NumMembers;
#else
  struct _TotalSizeComp {
    unsigned long u4Lo;
    unsigned long u4Hi;
  } TotalSizeComp;
  struct _TotalSize {
    unsigned long u4Lo;
    unsigned long u4Hi;
  } TotalSize;
  struct _NumMembers {
    unsigned long u4Lo;
    unsigned long u4Hi;
  } NumMembers;
#endif
  unsigned CompFactor;
  WORD cchComment;
} USERFUNCTIONS, far * LPUSERFUNCTIONS;

/* The following symbol UZ_DCL_STRUCTVER must be incremented whenever an
 * incompatible change is applied to the WinDLL API structure "DCL" !
 */
#define UZ_DCL_STRUCTVER        0x600
/* The structure "DCL" is collects most the UnZip WinDLL program options
 * that control the operation of the main UnZip WinDLL function.
 */
typedef struct {
  unsigned StructVersID;  /* struct version id (= UZ_DCL_STRUCTVER) */
  int ExtractOnlyNewer;   /* TRUE for "update" without interaction
                             (extract only newer/new files, without queries) */
  int SpaceToUnderscore;  /* TRUE if convert space to underscore */
  int PromptToOverwrite;  /* TRUE if prompt to overwrite is wanted */
  int fQuiet;             /* quiet flag:
                             { 0 = all | 1 = few | 2 = no } messages */
  int ncflag;             /* write to stdout if TRUE */
  int ntflag;             /* test zip file */
  int nvflag;             /* verbose listing */
  int nfflag;             /* "freshen" (replace existing files by newer versions) */
  int nzflag;             /* display zip file comment */
  int ndflag;             /* controls (sub)dir recreation during extraction
                             0 = junk paths from filenames
                             1 = "safe" usage of paths in filenames (skip ../)
                             2 = allow unsafe path components (dir traversal)
                           */
  int noflag;             /* always overwriting existing files if TRUE */
  int naflag;             /* do end-of-line translation */
  int nZIflag;            /* get ZipInfo output if TRUE */
  int B_flag;             /* backup existing files if TRUE */
  int C_flag;             /* be case insensitive if TRUE */
  int D_flag;             /* controls restoration of timestamps
                             0 = restore all timestamps (default)
                             1 = skip restoration of timestamps for folders
                                 created on behalf of directory entries in the
                                 Zip archive
                             2 = no restoration of timestamps; extracted files
                                 and dirs get stamped with current time */
  int U_flag;             /* controls UTF-8 filename coding support
                             0 = automatic UTF-8 translation enabled (default)
                             1 = recognize UTF-8 coded names, but all non-ASCII
                                 characters are "escaped" into "#Uxxxx"
                             2 = UTF-8 support is disabled, filename handling
                                 works exactly as in previous UnZip versions */
  int fPrivilege;         /* 1 => restore ACLs in user mode,
                             2 => try to use privileges for restoring ACLs */
  LPSTR lpszZipFN;        /* zip file name */
  LPSTR lpszExtractDir;   /* directory to extract to. This should be NULL if
                             you are extracting to the current directory. */
} DCL, far * LPDCL;

#ifdef __cplusplus
}
#endif

/* return codes of the (DLLPASSWORD)() callback function */
#define IDM_REPLACE_NO     100
#define IDM_REPLACE_TEXT   101
#define IDM_REPLACE_YES    102
#define IDM_REPLACE_ALL    103
#define IDM_REPLACE_NONE   104
#define IDM_REPLACE_RENAME 105
#define IDM_REPLACE_HELP   106

#endif /* __unzip_structs_h */
