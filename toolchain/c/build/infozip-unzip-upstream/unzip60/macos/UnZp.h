/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  UnZp.h

  This header-files is global to the project Unzip standalone.

  ---------------------------------------------------------------------------*/

/*****************************************************************************/
/*  Macros, typedefs                                                         */
/*****************************************************************************/

#define MACUNZIP_STANDALONE
#define MACUNZIP

/* These functions are defined as a macro instead of a function.
so we have to undefine them for replacing */
#undef getc
#undef getchar
#undef putchar
#undef putc

#ifndef    TRUE
#define    TRUE 1
#endif  /* TRUE */
#ifndef    FALSE
#define    FALSE 0
#endif  /* FALSE */

/*****************************************************************************/
/*  Includes standard headers                                                */
/*****************************************************************************/
#include <ansi_prefix.mac.h>
#include <TextUtils.h>
#include <Folders.h>
#include <Aliases.h>
#include <Resources.h>
#include <Gestalt.h>
#include <Traps.h>
#include <Processes.h>
#include <MacWindows.h>
#include <Fonts.h>
#include <ToolUtils.h>
#include <Dialogs.h>
#include <Devices.h>
#include <StandardFile.h>




/*
#define MAC_DEBUG  1
 */


#ifdef MAC_DEBUG
#define LOG_DEBUG   7   /* debug-level messages */
int Print2Syslog(UInt8 priority, const char *format, ...);
#include <ctype.h>


#define Notify(msg)                                             \
    {                                                           \
    (void)Print2Syslog(LOG_DEBUG, "%s (file: %s line: %d)",     \
                       msg, __FILE__, __LINE__);                \
    }



#define Assert_it(cond,msg,kind)                                    \
    {                                                               \
    if (!(cond))                                                    \
        {                                                           \
        (void)Print2Syslog(LOG_DEBUG, "%s failed: [%s] cond: [%s] (file: %s line: %d)", \
                           kind, msg, #cond, __FILE__, __LINE__);   \
        }                                                           \
    }



#define AssertBool(b,msg) \
    Assert_it (((b) == TRUE) || ((b) == FALSE),(msg),("AssertBool "))



#define AssertStr(s,msg)                                            \
    {                                                               \
        int s_i = 0;                                                \
        Assert_it ((s),(msg),("1. AssertStr "));                    \
        while ((s)[s_i]) {                                          \
            Assert_it ((!iscntrl((s)[s_i]) || ((s)[s_i] == 0x0A) || \
                       ((s)[s_i] == 0x0D)),(s),("2. AssertStr "));  \
            s_i++;                                                  \
        }                                                           \
    }



#define AssertTime(t,msg) \
    Assert_it (((t).tm_sec  >=  0) && ((t).tm_sec  < 62) &&   \
               ((t).tm_min  >=  0) && ((t).tm_min  < 60) &&   \
               ((t).tm_hour >=  0) && ((t).tm_hour < 24) &&   \
               ((t).tm_mday >=  1) && ((t).tm_mday < 32) &&   \
               ((t).tm_mon  >=  0) && ((t).tm_mon  < 12) &&   \
               ((t).tm_wday >=  0) && ((t).tm_wday < 7)  &&   \
               ((t).tm_yday >=  0) && ((t).tm_yday < 366),(msg),("AssertStr "))



#define AssertIntRange(myvalue,minimum,maximum, msg) \
    Assert_it (((myvalue) >= (minimum)) && ((myvalue) <= (maximum)), \
               msg,("AssertIntRange "))



#define AssertStrNoOverlap(str1,str2,msg)                           \
    {                                                               \
        long s_i = 0;                                               \
        AssertStr((str1),(msg))                                     \
        AssertStr((str2),(msg))                                     \
        if ((str1) < (str2))                                        \
            {                                                       \
            s_i = strlen((str2));                                   \
            Assert_it ( (((str1) + s_i) < (str2)),(msg),("AssertStrNoOverlap "));   \
            }                                                       \
        else                                                        \
            {                                                       \
            s_i = strlen((str1));                                   \
            Assert_it ( (((str2) + s_i) < (str1)),(msg),("AssertStrNoOverlap "));   \
            }                                                       \
    }                                                               \




#else /* !MAC_DEBUG */
#define Assert_it(cond,msg,kind)
#define AssertBool(b,msg)
#define AssertStr(s,msg)
#define AssertTime(t,msg)
#define AssertIntRange(myvalue,minimum,maximum,msg)
#define AssertStrNoOverlap(str1,str2,msg)
#endif /* ?MAC_DEBUG */
