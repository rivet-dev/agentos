On Windows open this file in WordPad.

Contents of the "windll/csharp" sub-archive

This directory contains Visual C Sharp (C#) sample project for
using the unzip32.dll library.  This project was generously donated
by Adrian Maull.  The Zip source archive contains a corresponding
project for using the zip32.dll library.

These sample projects are distributed as part of the Info-ZIP distribution
under the Info-ZIP license.

Note that the files may be saved in "UNIX LF" format with carriage returns
stripped.  These may need to be restored before the project can be successfully
used.

The project is based on .NET Framework 1.1.  It was contributed
to the Info-Zip project April 2005.  If you have questions or comments,
contact us at www.Info-ZIP.org or for specific questions about these
projects contact Adrian Maull directly at adrian.maull@sprintpcs.com.

See UnZip.cs for more detailed information about the project.  Currently
the sample project has some bugs as noted in the above file.

The original code has been adapted to the modified WinDLL interface of
UnZip 6, using Visual C# 2005 (.Net Framework 2.0) and a unzip32.dll compiled
by Visual C++ 6.0.  The provided project file is still in the format for
Visual Studio 7.1 (VS .Net 2003, .Net 1.1).  But the code of the project
can be used with newer Visual Studio versions (2005 = 8.0 or 2008 = 9.0);
only the project file gets (irreversibly) converted to the newer Visual
Studio format when first opened.

However, this project is not tested throughoutly by us at this time.

Note on using Visual Studio C# 2005, 2008 (or newer?):
The UnZip maintainer was unsuccessful to run the C# sample code using
Visual Studio 2005 or 2008 together with an unzip32.dll compiled with
the same Visual Studio version in default configuration (unzip32.dll linked
against the DLL version of the C runtime library).  The C# program failed
to load the unzip32.dll because of an "OS loader lock" conflict. This conflict
should only show up when loading "mixed mode" dll that contain both managed
and unmanaged code.  So, it cannot be caused by the "pure native code"
unzip32.dll directly, which contains nothing dangerous in its DLLMain()
function.  It seems to be caused by the new "side-by-side" C runtime dlls
(msvcr80.dll or msvcr90.dll being loaded multiple times by a .NET application
that uses msvcr??-linked native dlls).
To fix this issue, one of the following work-arounds could be used:
a) use an unzip32.dll that was linked against the system msvcrt.dll (the
   "old" MSVC 6.0 runtime) that is a core system component of all Windows
   versions from Windows 98/Windows ME and Windows 2000 and newer (at least
   up to Windows Vista/Windows Server 2008),
or
b) use a statically linked variant of unzip32.dll (see the option DLLSTANDALONE
   in the MSC Makefile for Win32 executables, "win32/Makefile").

Ed Gordon, Christian Spieler
2009/01/17
