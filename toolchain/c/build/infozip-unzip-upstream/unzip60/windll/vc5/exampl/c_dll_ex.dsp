# Microsoft Developer Studio Project File - Name="c_dll_ex" - Package Owner=<4>
# Microsoft Developer Studio Generated Build File, Format Version 5.00
# ** DO NOT EDIT **

# TARGTYPE "Win32 (x86) Console Application" 0x0103

CFG=c_dll_ex - Win32 Debug
!MESSAGE This is not a valid makefile. To build this project using NMAKE,
!MESSAGE use the Export Makefile command and run
!MESSAGE 
!MESSAGE NMAKE /f "c_dll_ex.mak".
!MESSAGE 
!MESSAGE You can specify a configuration when running NMAKE
!MESSAGE by defining the macro CFG on the command line. For example:
!MESSAGE 
!MESSAGE NMAKE /f "c_dll_ex.mak" CFG="c_dll_ex - Win32 Debug"
!MESSAGE 
!MESSAGE Possible choices for configuration are:
!MESSAGE 
!MESSAGE "c_dll_ex - Win32 Release" (based on "Win32 (x86) Console Application")
!MESSAGE "c_dll_ex - Win32 Debug" (based on "Win32 (x86) Console Application")
!MESSAGE "c_dll_ex - Win32 Static Release" (based on "Win32 (x86) Console Application")
!MESSAGE "c_dll_ex - Win32 Static Debug" (based on "Win32 (x86) Console Application")
!MESSAGE 

# Begin Project
# PROP Scc_ProjName ""
# PROP Scc_LocalPath ""
CPP=cl.exe
RSC=rc.exe

!IF  "$(CFG)" == "c_dll_ex - Win32 Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "../Release/app"
# PROP Intermediate_Dir "Release"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /MD /W3 /GX /O2 /D "NDEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /FD /c
# ADD CPP /nologo /MD /W3 /GX /O2 /D "NDEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /machine:I386
# ADD LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /machine:I386 /out:"../Release/app/uzexampl.exe"

!ELSEIF  "$(CFG)" == "c_dll_ex - Win32 Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "../Debug/app"
# PROP Intermediate_Dir "Debug"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
# ADD BASE CPP /nologo /MDd /W3 /Gm /GX /Zi /Od /D "_DEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /FD /GZ /c
# ADD CPP /nologo /MDd /W3 /Gm /GX /Zi /Od /D "_DEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /YX /FD /GZ /c
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /debug /machine:I386 /pdbtype:sept
# ADD LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /debug /machine:I386 /out:"../Debug/app/uzexampl.exe" /pdbtype:sept

!ELSEIF  "$(CFG)" == "c_dll_ex - Win32 Static Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "StatRelease"
# PROP BASE Intermediate_Dir "StatRelease"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "../StatRelease/app"
# PROP Intermediate_Dir "StatRelease"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /MT /W3 /GX /O2 /D "NDEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /FD /c
# ADD CPP /nologo /MT /W3 /GX /O2 /D "NDEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /YX /FD /c
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /machine:I386
# ADD LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /machine:I386 /out:"../StatRelease/app/uzexampl.exe"

!ELSEIF  "$(CFG)" == "c_dll_ex - Win32 Static Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "StatDebug"
# PROP BASE Intermediate_Dir "StatDebug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "../StatDebug/app"
# PROP Intermediate_Dir "StatDebug"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
# ADD BASE CPP /nologo /MTd /W3 /Gm /GX /Zi /Od /D "_DEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /FD /GZ /c
# ADD CPP /nologo /MTd /W3 /Gm /GX /Zi /Od /D "_DEBUG" /D "_CONSOLE" /D "_MBCS" /D "WIN32" /YX /FD /GZ /c
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /debug /machine:I386 /pdbtype:sept
# ADD LINK32 kernel32.lib user32.lib advapi32.lib version.lib /nologo /subsystem:console /debug /machine:I386 /out:"../StatDebug/app/uzexampl.exe" /pdbtype:sept

!ENDIF 

# Begin Target

# Name "c_dll_ex - Win32 Release"
# Name "c_dll_ex - Win32 Debug"
# Name "c_dll_ex - Win32 Static Release"
# Name "c_dll_ex - Win32 Static Debug"
# Begin Group "Source Files"

# PROP Default_Filter "cpp;c;cxx;rc;def;r;odl;idl;hpj;bat"
# Begin Source File

SOURCE=..\..\uzexampl.c
# End Source File
# End Group
# Begin Group "Header Files"

# PROP Default_Filter "h;hpp;hxx;hm;inl"
# Begin Source File

SOURCE=..\..\uzexampl.h
# End Source File
# End Group
# End Target
# End Project
