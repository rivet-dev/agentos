# Microsoft Developer Studio Project File - Name="unzsfx32" - Package Owner=<4>
# Microsoft Developer Studio Generated Build File, Format Version 6.00
# ** DO NOT EDIT **

# TARGTYPE "Win32 (x86) Static Library" 0x0104

CFG=unzsfx32 - Win32 ASM Debug
!MESSAGE This is not a valid makefile. To build this project using NMAKE,
!MESSAGE use the Export Makefile command and run
!MESSAGE 
!MESSAGE NMAKE /f "unzsfx32.mak".
!MESSAGE 
!MESSAGE You can specify a configuration when running NMAKE
!MESSAGE by defining the macro CFG on the command line. For example:
!MESSAGE 
!MESSAGE NMAKE /f "unzsfx32.mak" CFG="unzsfx32 - Win32 ASM Debug"
!MESSAGE 
!MESSAGE Possible choices for configuration are:
!MESSAGE 
!MESSAGE "unzsfx32 - Win32 Release" (based on "Win32 (x86) Static Library")
!MESSAGE "unzsfx32 - Win32 Debug" (based on "Win32 (x86) Static Library")
!MESSAGE "unzsfx32 - Win32 ASM Release" (based on "Win32 (x86) Static Library")
!MESSAGE "unzsfx32 - Win32 ASM Debug" (based on "Win32 (x86) Static Library")
!MESSAGE 

# Begin Project
# PROP AllowPerConfigDependencies 0
# PROP Scc_ProjName ""
# PROP Scc_LocalPath ""
CPP=cl.exe
RSC=rc.exe

!IF  "$(CFG)" == "unzsfx32 - Win32 Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "../Release"
# PROP Intermediate_Dir "Release"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /Gz /W3 /GX /O2 /D "NDEBUG" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /Gz /W3 /GX /O1 /I "../../.." /D "NDEBUG" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "SFX" /D "USE_EF_UT_TIME" /YX /FD /c
# ADD BASE RSC /l 0x409
# ADD RSC /l 0x409
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo

!ELSEIF  "$(CFG)" == "unzsfx32 - Win32 Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "../Debug"
# PROP Intermediate_Dir "Debug"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /Gz /W3 /Gm /GX /Zi /Od /D "_DEBUG" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /Gz /W3 /Gm /GX /Zi /Od /I "../../.." /D "_DEBUG" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "SFX" /D "USE_EF_UT_TIME" /YX /FD /c
# ADD BASE RSC /l 0x409
# ADD RSC /l 0x409
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo

!ELSEIF  "$(CFG)" == "unzsfx32 - Win32 ASM Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "../Release.ASM"
# PROP Intermediate_Dir "Release.ASM"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /Gz /W3 /GX /O2 /D "NDEBUG" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /Gz /W3 /GX /O1 /I "../../.." /D "NDEBUG" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "SFX" /D "USE_EF_UT_TIME" /D "ASM_CRC" /YX /FD /c
# ADD BASE RSC /l 0x409
# ADD RSC /l 0x409
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo

!ELSEIF  "$(CFG)" == "unzsfx32 - Win32 ASM Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "../Debug.ASM"
# PROP Intermediate_Dir "Debug.ASM"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /Gz /W3 /Gm /GX /Zi /Od /D "_DEBUG" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD CPP /nologo /Gz /W3 /Gm /GX /Zi /Od /I "../../.." /D "_DEBUG" /D "_MBCS" /D "WIN32" /D "WINDLL" /D "DLL" /D "UNZIPLIB" /D "SFX" /D "USE_EF_UT_TIME" /D "ASM_CRC" /YX /FD /c
# ADD BASE RSC /l 0x409
# ADD RSC /l 0x409
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo

!ENDIF 

# Begin Target

# Name "unzsfx32 - Win32 Release"
# Name "unzsfx32 - Win32 Debug"
# Name "unzsfx32 - Win32 ASM Release"
# Name "unzsfx32 - Win32 ASM Debug"
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

SOURCE=..\..\..\win32\win32.c
# End Source File
# Begin Source File

SOURCE=..\..\..\win32\win32i64.c
# End Source File
# Begin Source File

SOURCE=..\..\..\windll\windll.c
# End Source File
# End Target
# End Project
