# Microsoft Developer Studio Project File - Name="unz32lib" - Package Owner=<4>
# Microsoft Developer Studio Generated Build File, Format Version 6.00
# ** DO NOT EDIT **

# TARGTYPE "Win32 (x86) Static Library" 0x0104

CFG=unz32lib - Win32 ASM Debug
!MESSAGE This is not a valid makefile. To build this project using NMAKE,
!MESSAGE use the Export Makefile command and run
!MESSAGE 
!MESSAGE NMAKE /f "unz32lib.mak".
!MESSAGE 
!MESSAGE You can specify a configuration when running NMAKE
!MESSAGE by defining the macro CFG on the command line. For example:
!MESSAGE 
!MESSAGE NMAKE /f "unz32lib.mak" CFG="unz32lib - Win32 ASM Debug"
!MESSAGE 
!MESSAGE Possible choices for configuration are:
!MESSAGE 
!MESSAGE "unz32lib - Win32 Release" (based on "Win32 (x86) Static Library")
!MESSAGE "unz32lib - Win32 Debug" (based on "Win32 (x86) Static Library")
!MESSAGE "unz32lib - Win32 ASM Release" (based on "Win32 (x86) Static Library")
!MESSAGE "unz32lib - Win32 ASM Debug" (based on "Win32 (x86) Static Library")
!MESSAGE 

# Begin Project
# PROP AllowPerConfigDependencies 0
# PROP Scc_ProjName ""
# PROP Scc_LocalPath ""
CPP=cl.exe
RSC=rc.exe

!IF  "$(CFG)" == "unz32lib - Win32 Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "../Release/libs"
# PROP Intermediate_Dir "Release"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /GX /O2 /D "NDEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /W3 /GX /O2 /I "../../.." /D "NDEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "USE_EF_UT_TIME" /FD /c
# SUBTRACT CPP /YX
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo /out:"../Release/libs/unzip32.lib"

!ELSEIF  "$(CFG)" == "unz32lib - Win32 Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "../Debug/libs"
# PROP Intermediate_Dir "Debug"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /GX /Z7 /Od /D "_DEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /W3 /GX /Z7 /Od /I "../../.." /D "_DEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "USE_EF_UT_TIME" /FD /c
# SUBTRACT CPP /YX
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo /out:"../Debug/libs/unzip32.lib"

!ELSEIF  "$(CFG)" == "unz32lib - Win32 ASM Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "../Release.ASM/libs"
# PROP Intermediate_Dir "Release.ASM"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /GX /O2 /D "NDEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /W3 /GX /O2 /I "../../.." /D "NDEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "USE_EF_UT_TIME" /D "ASM_CRC" /FD /c
# SUBTRACT CPP /YX
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo /out:"../Release.ASM/libs/unzip32.lib"

!ELSEIF  "$(CFG)" == "unz32lib - Win32 ASM Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "../Debug.ASM/libs"
# PROP Intermediate_Dir "Debug.ASM"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /GX /Z7 /Od /D "_DEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /W3 /GX /Z7 /Od /I "../../.." /D "_DEBUG" /D "_WINDOWS" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "USE_EF_UT_TIME" /D "ASM_CRC" /FD /c
# SUBTRACT CPP /YX
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo /out:"../Debug.ASM/libs/unzip32.lib"

!ENDIF 

# Begin Target

# Name "unz32lib - Win32 Release"
# Name "unz32lib - Win32 Debug"
# Name "unz32lib - Win32 ASM Release"
# Name "unz32lib - Win32 ASM Debug"
# Begin Source File

SOURCE=..\..\..\api.c
# End Source File
# Begin Source File

SOURCE=..\..\..\crc32.c
# End Source File
# Begin Source File

SOURCE=..\..\..\win32\crc_i386.c
# End Source File
# Begin Source File

SOURCE=..\..\..\crypt.c
# End Source File
# Begin Source File

SOURCE=..\..\..\explode.c
# End Source File
# Begin Source File

SOURCE=..\..\..\extract.c
# End Source File
# Begin Source File

SOURCE=..\..\..\fileio.c
# End Source File
# Begin Source File

SOURCE=..\..\..\globals.c
# End Source File
# Begin Source File

SOURCE=..\..\..\inflate.c
# End Source File
# Begin Source File

SOURCE=..\..\..\list.c
# End Source File
# Begin Source File

SOURCE=..\..\..\match.c
# End Source File
# Begin Source File

SOURCE=..\..\..\win32\nt.c
# End Source File
# Begin Source File

SOURCE=..\..\..\process.c
# End Source File
# Begin Source File

SOURCE=..\..\..\ubz2err.c
# End Source File
# Begin Source File

SOURCE=..\..\..\unreduce.c
# End Source File
# Begin Source File

SOURCE=..\..\..\unshrink.c
# End Source File
# Begin Source File

SOURCE=..\..\..\windll\unziplib.def
# End Source File
# Begin Source File

SOURCE=..\..\..\win32\win32.c
# End Source File
# Begin Source File

SOURCE=..\..\..\win32\win32i64.c
# End Source File
# Begin Source File

SOURCE=..\..\..\windll\windll.c
# End Source File
# Begin Source File

SOURCE=..\..\..\zipinfo.c
# End Source File
# End Target
# End Project
