#ifndef __MCBINIII_H__
#define __MCBINIII_H__


/****************************************************************
    MacBinaryIII.h

    Copyright 1997 Christopher Evans (cevans@poppybank.com)

    Basic encoding and decoding of Macintosh files to the
    MacBinary III spec.
****************************************************************/


/*
--------------------------------------------------------------------
This source is copyrighted by Christopher Evans (cevans@poppybank.com)
(available at ftp://ftp.lazerware.com/MacBinaryIII_src_C.sit
 homepage of Leonard Rosenthol  leonardr@netcom.com)



I made the following changes:
    * renamed files MacBinaryIII.h / MacBinaryIII.c to
      macbin3.c / macbin3.h
    * removed pragmas:
        #pragma once
        #pragma options align=mac68k
    * added 5 macros to make the migration into other projects easier:
        DELETE_MACBINARY_SOURCE  -> delete the macbinary file after
                                    decoding
        INCLUDE_ENCODE_MACBINARY -> include the encode-part of the source
        INCLUDE_DECODE_MACBINARY -> include the decode-part of the source
        INCLUDE_CRC32CALC        -> include the CRC32 calculation
        TRACE_MACBINARY          -> produce some diagnostic printouts
    * changed the source layout to fit into a 80 col window
    * removed non-ASCII characters from the sources
    * renamed the function CalculateCRC() to crc32() and changed
      the order of the arguments. So the crc32() func of the InfoZip
      package can be used instead.
    * moved module specific macros and protos to the mcbiniii.c
    * added comments and enhanced the documention of the MacBinary Spec
    * changed definition crc_table[] from short to unsigned long.
    * improved the HeaderIsMacBinary() function to be more restrictive

The specification says:
    "Offsets 83 and 87, Long Word, (the length of the forks)
    should be in the range of 0-$007FFFFF "
However, I do not understand the reason of this range. The data fork
can be much larger than $007FFFFF ( 2 Gigabytes! ) and the size of the
resource fork can be until 16 MByte. ZipIt and Stuffit seems to be
ignore this limit.
I changed this limit (see kResourceForkMaxLen), and I removed the
maximum check of the data fork.

Dirk Haase (d_haase@sitec.de); 05.Dec.1998

--------------------------------------------------------------------



MacBinary III

MacBinary is a standard format for binary transfer of arbitrary Macintosh
documents via a telecommunication link. It is intended for use both between
Macintoshes and for use in uploading arbitrary Macintosh documents to remote
systems (where it is presumed that they will be stored as an exact image of
the data transmitted). It does this by combing both the resource and data
forks (as well as the "Finder Info") of a standard Macintosh file into a
single data fork only file that can be stored on non-Macintosh machines.


The format of the header for MacBinary III is as follows:
 Offset   Length         Contents
 000      Byte           old version number, must be kept at zero for
                         compatibility
 001      Byte           Length of filename (must be in the range 1-31)
 002      1 to 63 Bytes  filename (only "length" bytes are significant).
 065      Long Word      file type (normally expressed as four characters)
 069      Long Word      file creator (normally expressed as four
                         characters)
 073      Byte           original Finder flags
                         Bit 7 - isAlias.
                         Bit 6 - isInvisible.
                         Bit 5 - hasBundle.
                         Bit 4 - nameLocked.
                         Bit 3 - isStationery.
                         Bit 2 - hasCustomIcon.
                         Bit 1 - reserved.
                         Bit 0 - hasBeenInited.
 074      Byte           zero fill, must be zero for compatibility
 075      Word           file's vertical position within its window.
 077      Word           file's horizontal position within its window.
 079      Word           file's window or folder ID.
 081      Byte           "Protected" flag (in low order bit).
 082      Byte           zero fill, must be zero for compatibility
 083      Long Word      Data Fork length (bytes, zero if no Data Fork).
 087      Long Word      Resource Fork length (bytes, zero if no R.F.).
 091      Long Word      File's creation date
 095      Long Word      File's "last modified" date.
 099      Word           length of Get Info comment to be sent after the
                         resource fork (if implemented, see below).
 101      Byte           Finder Flags, bits 0-7. (Bits 8-15 are already in
                         byte 73)
                         Bit 7    - hasNoInits
                         Bit 6    - isShared
                         Bit 5    - requiresSwitchLaunch
                         Bit 4    - ColorReserved
                         Bits 1-3 - color
                         Bit 0    - isOnDesk
 *102     Long Word      signature for indentification purposes ('mBIN')
 *106     Byte           script of file name (from the fdScript field of an
                         fxInfo record)
 *107     Byte           extended Finder flags (from the fdXFlags field of
                         an fxInfo record)
 108-115                 Unused (must be zeroed by creators, must be
                         ignored by readers)
 116      Long Word      Length of total files when packed files are
                         unpacked. As of the writing of this document, this
                         field has never been used.
 120      Word           Length of a secondary header. If this is non-zero,
                         skip this many bytes (rounded up to the next
                         multiple of 128). This is for future expansion
                         only, when sending files with MacBinary, this word
                         should be zero.
 *122     Byte           Version number of MacBinary III that the uploading
                         program is written for (the version is 130 for
                         MacBinary III)
 123      Byte           Minimum MacBinary version needed to read this file
                         (set this value at 129 for backwards compatibility
                         with MacBinary II)
 124      Word           CRC of previous 124 bytes

*These fields have changed for MacBinary III.

   All values are stored in normal 68000 order, with Most Significant Byte
appearing first then the file. Any bytes in the header not defined above
should be set to zero.

*/


/*
Public functions
*/

OSErr   EncodeMacbinaryFile(FSSpec *file);
OSErr   DecodeMacBinaryFile(FSSpec *source);
Boolean FSpIsMacBinary(FSSpec *file);


#endif

