/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  wcemain.c

  ---------------------------------------------------------------------------*/

#define __WCEMAIN_C     /* identifies this source module */
#define UNZIP_INTERNAL
#include "unzip.h"      /* includes, typedefs, macros, prototypes, etc. */


int WINAPI WinMain( HINSTANCE hInstance,
                    HINSTANCE hPrevInstance,
                    LPTSTR    lpCmdLine,
                    int       nCmdShow)
{
    int r;
    int i;
    LPTSTR argArray[10];
    int argBuffSize = lstrlen(lpCmdLine) + 1;
    void* argBuff = malloc(argBuffSize);
    char* argv[10];


    /* Parse the command line into an argument array */
    int argc = 0;
    LPTSTR argPtr = lpCmdLine;
    LPTSTR nextParam = NULL;
    LPTSTR closingQuote = NULL;
    unsigned short* endOfCmdLine = lpCmdLine + lstrlen(lpCmdLine);
    TCHAR Blank = _T(' ');

    /* Init the arrays */
    for (i = 0; i < 10;i++)
        argArray[i];
    for (i = 0;i < 10;i++)
        argv[i] = NULL;


    /* Create the argument array, we have to convert this from wchar
     * (unicode) to mbcs (single byte ascii)
     */
    while(argPtr != NULL)
    {
        /* Look for the first non-blank character */
        while((memcmp(argPtr, &Blank, sizeof(TCHAR)) == 0) &&
              (argPtr < endOfCmdLine))
        {
            argPtr++;
        }

        /* Check for quote enclosed strings for extended file names. */
        if (argPtr[0] == _T('"') &&
            /* Look for closing quote */
            (closingQuote = _tcschr(argPtr + 1, _T('"'))) != NULL)
        {
            /* Clear the enclosing quotes */
            *argPtr++ = _T('\0');
            *closingQuote = _T('\0');
            nextParam = closingQuote + 1;
        }
        else
        {
            nextParam = argPtr;
        }

        /* Set the parameter */
        argArray[argc] = argPtr;
        argc++;

        /* Look for the next blank */
        argPtr = _tcschr(nextParam, _T(' '));
        if (argPtr != NULL)
        {
            /* Terminate the parameter and
               point after the blank to the keyword */
            *argPtr++ = _T('\0');
        }
    }
    /* Add one to the arg count for null terminator. */
    argc++;

#ifndef UNICODE
    /* No need to convert the parameters */
    argv = (char*)argArray;
#else
    /* Unicode so we need to convert the parameters to ascii. */
    /* Get the storage we need to hold the converted data */
    if (argBuff != NULL)
    {

        int i;
        char* ptrArgBuff;

        /* Clear the asci argument buffer */
        memset(argBuff,'\0',argBuffSize);
        /* Command line parameters give ? */
        if (argBuffSize == 0)
        {
            /* No so just set the first argumen in the array to
             * the buffer */
            argv[0] = argBuff;
        }
        else
        {
            argv[0] = (char*)_T("UnzipCE.exe");
            /* We have some storage build an asci version of
             * the command parameters */
            ptrArgBuff = (char*)argBuff;
            for(i = 0; i < (argc - 1); i++)
            {
                /* convert the data */
                int paramLength = lstrlen(argArray[i]);
                int bytesWritten =
                    WideCharToMultiByte(CP_ACP, 0,
                                        argArray[i], paramLength,
                                        ptrArgBuff, paramLength,
                                        NULL, NULL);
                if (bytesWritten == 0)
                {
                    /* Conversion failed ser return value and exit loop */
                    r = (int)GetLastError();
                    break;
                }
                else
                {
                    argv[i + 1] = ptrArgBuff;
                    /* Point to the next area of the buffer */
                    ptrArgBuff = ptrArgBuff + bytesWritten + 1;
                }
            }
        }
    }
#endif /* UNICODE */
    return MAIN(argc, argv);
}
