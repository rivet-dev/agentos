# Microsoft Developer Studio Project File - Name="punzip" - Package Owner=<4>
# Microsoft Developer Studio Generated Build File, Format Version 5.00
# ** DO NOT EDIT **

# TARGTYPE "Win32 (x86) Application" 0x0101
# TARGTYPE "Win32 (WCE MIPS) Application" 0x0a01
# TARGTYPE "Win32 (WCE SH) Application" 0x0901

CFG=punzip - Win32 Debug
!MESSAGE This is not a valid makefile. To build this project using NMAKE,
!MESSAGE use the Export Makefile command and run
!MESSAGE 
!MESSAGE NMAKE /f "punzip.mak".
!MESSAGE 
!MESSAGE You can specify a configuration when running NMAKE
!MESSAGE by defining the macro CFG on the command line. For example:
!MESSAGE 
!MESSAGE NMAKE /f "punzip.mak" CFG="punzip - Win32 Debug"
!MESSAGE 
!MESSAGE Possible choices for configuration are:
!MESSAGE 
!MESSAGE "punzip - Win32 Release" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 Debug" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 ANSI Release" (based on\
 "Win32 (x86) Application")
!MESSAGE "punzip - Win32 ANSI Debug" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 (WCE MIPS) Release" (based on\
 "Win32 (WCE MIPS) Application")
!MESSAGE "punzip - Win32 (WCE MIPS) Debug" (based on\
 "Win32 (WCE MIPS) Application")
!MESSAGE "punzip - Win32 (WCE SH) Release" (based on\
 "Win32 (WCE SH) Application")
!MESSAGE "punzip - Win32 (WCE SH) Debug" (based on\
 "Win32 (WCE SH) Application")
!MESSAGE 

# Begin Project
# PROP Scc_ProjName ""
# PROP Scc_LocalPath ""

!IF  "$(CFG)" == "punzip - Win32 Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "Release"
# PROP Intermediate_Dir "Release"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /W3 /GX /O2 /D "WIN32" /D "NDEBUG" /D "_WINDOWS" /YX /FD /c
# ADD CPP /nologo /MT /W3 /GX /O2 /I "../.." /D "NDEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "NDEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "NDEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG" /d "_X86_" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib gdi32.lib winspool.lib comdlg32.lib advapi32.lib shell32.lib ole32.lib oleaut32.lib uuid.lib odbc32.lib odbccp32.lib /nologo /subsystem:windows /machine:I386
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /machine:I386

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "Debug"
# PROP Intermediate_Dir "Debug"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /W3 /Gm /GX /Zi /Od /D "WIN32" /D "_DEBUG" /D "_WINDOWS" /YX /FD /c
# ADD CPP /nologo /MTd /W3 /Gm /GX /Zi /Od /I "../.." /D "_DEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "_DEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "_DEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG" /d "_X86_" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib gdi32.lib winspool.lib comdlg32.lib advapi32.lib shell32.lib ole32.lib oleaut32.lib uuid.lib odbc32.lib odbccp32.lib /nologo /subsystem:windows /debug /machine:I386 /pdbtype:sept
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /debug /machine:I386 /pdbtype:sept

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "punzip__"
# PROP BASE Intermediate_Dir "punzip__"
# PROP BASE Ignore_Export_Lib 0
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "AnsiRelease"
# PROP Intermediate_Dir "AnsiRelease"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /MT /W3 /GX /O2 /I "../.." /D "NDEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /YX /FD /c
# SUBTRACT BASE CPP /X /Fr
# ADD CPP /nologo /MT /W3 /GX /O2 /I "../.." /D "NDEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "NDEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "NDEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "NDEBUG" /d "_X86_"
# ADD RSC /l 0x409 /d "NDEBUG" /d "_X86_"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /machine:I386
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /machine:I386

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "punzip_0"
# PROP BASE Intermediate_Dir "punzip_0"
# PROP BASE Ignore_Export_Lib 0
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "AnsiDebug"
# PROP Intermediate_Dir "AnsiDebug"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /MTd /W3 /Gm /GX /Zi /Od /I "../.." /D "_DEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "UNICODE" /YX /FD /c
# SUBTRACT BASE CPP /X /Fr
# ADD CPP /nologo /MTd /W3 /Gm /GX /Zi /Od /I "../.." /D "_DEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "_DEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "_DEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "_DEBUG" /d "_X86_"
# ADD RSC /l 0x409 /d "_DEBUG" /d "_X86_"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /debug /machine:I386 /pdbtype:sept
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /debug /machine:I386 /pdbtype:sept

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "WMIPSRel"
# PROP BASE Intermediate_Dir "WMIPSRel"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "WMIPSRel"
# PROP Intermediate_Dir "WMIPSRel"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=clmips.exe
# ADD BASE CPP /nologo /W3 /O2 /D "NDEBUG" /D "MIPS" /D "_MIPS_" /D "_WIN32_WCE" /D "UNICODE" /YX /QMRWCE /c
# ADD CPP /nologo /W3 /GX /O2 /I "../.." /I "../../wince/inc" /D "NDEBUG" /D "MIPS" /D "_MIPS_" /D _WIN32_WCE=100 /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /QMRWCE /c
# SUBTRACT CPP /X /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d "_WIN32_WCE" /d "NDEBUG"
# ADD RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d "_WIN32_WCE" /d "NDEBUG" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libc.lib /nologo /machine:MIPS /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /machine:MIPS /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /nodefaultlib
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "WMIPSDbg"
# PROP BASE Intermediate_Dir "WMIPSDbg"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "WMIPSDbg"
# PROP Intermediate_Dir "WMIPSDbg"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=clmips.exe
# ADD BASE CPP /nologo /W3 /Zi /Od /D "DEBUG" /D "MIPS" /D "_MIPS_" /D "_WIN32_WCE" /D "UNICODE" /YX /QMRWCE /c
# ADD CPP /nologo /W3 /GX /Zi /Od /I "../.." /I "../../wince/inc" /D "DEBUG" /D "MIPS" /D "_MIPS_" /D _WIN32_WCE=100 /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /QMRWCE /c
# SUBTRACT CPP /X /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d "_WIN32_WCE" /d "DEBUG"
# ADD RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d "_WIN32_WCE" /d "DEBUG" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libcd.lib /nologo /debug /machine:MIPS /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /debug /machine:MIPS /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /nodefaultlib
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "WCESHRel"
# PROP BASE Intermediate_Dir "WCESHRel"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "WCESHRel"
# PROP Intermediate_Dir "WCESHRel"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=shcl.exe
# ADD BASE CPP /nologo /W3 /O2 /D "NDEBUG" /D "SH3" /D "_SH3_" /D "_WIN32_WCE" /D "UNICODE" /YX /c
# ADD CPP /nologo /W3 /GX /O2 /I "../.." /I "../../wince/inc" /D "NDEBUG" /D "SH3" /D "_SH3_" /D _WIN32_WCE=100 /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /c
# SUBTRACT CPP /X /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "SH3" /d "_SH3_" /d "_WIN32_WCE" /d "NDEBUG"
# ADD RSC /l 0x409 /r /d "SH3" /d "_SH3_" /d "_WIN32_WCE" /d "NDEBUG" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libc.lib /nologo /machine:SH3 /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /machine:SH3 /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /nodefaultlib
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "WCESHDbg"
# PROP BASE Intermediate_Dir "WCESHDbg"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "WCESHDbg"
# PROP Intermediate_Dir "WCESHDbg"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=shcl.exe
# ADD BASE CPP /nologo /W3 /Zi /Od /D "DEBUG" /D "SH3" /D "_SH3_" /D "_WIN32_WCE" /D "UNICODE" /YX /c
# ADD CPP /nologo /W3 /GX /Zi /Od /I "../.." /I "../../wince/inc" /D "DEBUG" /D "SH3" /D "_SH3_" /D _WIN32_WCE=100 /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /c
# SUBTRACT CPP /X /u /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "SH3" /d "_SH3_" /d "_WIN32_WCE" /d "DEBUG"
# ADD RSC /l 0x409 /r /d "SH3" /d "_SH3_" /d "_WIN32_WCE" /d "DEBUG" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libcd.lib /nologo /debug /machine:SH3 /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /debug /machine:SH3 /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /map /nodefaultlib
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY

!ENDIF 

# Begin Target

# Name "punzip - Win32 Release"
# Name "punzip - Win32 Debug"
# Name "punzip - Win32 ANSI Release"
# Name "punzip - Win32 ANSI Debug"
# Name "punzip - Win32 (WCE MIPS) Release"
# Name "punzip - Win32 (WCE MIPS) Debug"
# Name "punzip - Win32 (WCE SH) Release"
# Name "punzip - Win32 (WCE SH) Debug"
# Begin Group "Info-ZIP Sources"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\..\api.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\consts.h
# End Source File
# Begin Source File

SOURCE=..\..\crc32.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\crc32.h
# End Source File
# Begin Source File

SOURCE=..\..\crypt.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\crypt.h
# End Source File
# Begin Source File

SOURCE=..\..\ebcdic.h
# End Source File
# Begin Source File

SOURCE=..\..\explode.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\extract.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\fileio.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\globals.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\globals.h
# End Source File
# Begin Source File

SOURCE=..\..\inflate.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\inflate.h
# End Source File
# Begin Source File

SOURCE=..\..\list.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\process.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\ttyio.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\ttyio.h
# End Source File
# Begin Source File

SOURCE=..\..\ubz2err.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_UBZ2ERR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_UBZ2ERR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_UBZ2ERR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_UBZ2ERR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\unreduce.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\unshrink.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\unzip.h
# End Source File
# Begin Source File

SOURCE=..\..\unzpriv.h
# End Source File
# Begin Source File

SOURCE=..\..\unzvers.h
# End Source File
# Begin Source File

SOURCE=..\..\zip.h
# End Source File
# End Group
# Begin Group "Pocket UnZip Sources"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\intrface.cpp

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\intrface.h
# End Source File
# Begin Source File

SOURCE=..\punzip.h
# End Source File
# Begin Source File

SOURCE=..\punzip.rc

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\punzip.rcv
# End Source File
# Begin Source File

SOURCE=..\resource.h
# End Source File
# Begin Source File

SOURCE=..\wcecfg.h
# End Source File
# Begin Source File

SOURCE=..\wince.cpp

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\wince.h
# End Source File
# Begin Source File

SOURCE=..\winmain.cpp

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Release"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH) Debug"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\winmain.h
# End Source File
# End Group
# Begin Group "Resources"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\ilmask.bmp
# End Source File
# Begin Source File

SOURCE=..\imglist.2bp
# End Source File
# Begin Source File

SOURCE=..\imglist.bmp
# End Source File
# Begin Source File

SOURCE=..\punzip.ic2
# End Source File
# Begin Source File

SOURCE=..\punzip.ico
# End Source File
# Begin Source File

SOURCE=..\toolbar.2bp
# End Source File
# Begin Source File

SOURCE=..\toolbar.bmp
# End Source File
# Begin Source File

SOURCE=..\zipfile.ic2
# End Source File
# Begin Source File

SOURCE=..\zipfile.ico
# End Source File
# End Group
# Begin Group "Documentation"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\Contents
# End Source File
# Begin Source File

SOURCE=..\punzip.htp
# End Source File
# Begin Source File

SOURCE=..\README
# End Source File
# End Group
# End Target
# End Project
