On Windows, please read this file using WordPad to format lines properly.

This directory contains a short example on how to use the Win32 UnZip DLL
with Visual Basic.  The example code consists of a simple dialog form and
a standard module containing the interface code to call unzip32.dll.
The code assumes that the unzip32.dll binary is available somewhere on the
execution path. The easiest way to satisfy this rule is to keep a copy of
unzip32.dll in the directory where the VB executable is stored, but if
it's in the command path it should be found.
NOTE: Whenever a VB program is run from the VB6 IDE, the "path of the VB
executable" is the path where the IDE program file "VB6.EXE" is stored,
NOT the storage directory of the VB project file (= result of the App.Path
property accessed from within the VB program).

The example code has been edited last with Visual Basic 6, but should be
compatible with VB 5. To maintain compatibility with VB5 after having modified
the example project in VB6, the project file has to be edited using a standard
text editor (e.g.: Notepad), to remove the line specifying the "Retained"
property.  This property line is not recognized by VB 5 and would prevent
correct loading of the project in VB 5.

This VB example makes use of the "Windows Common Dialogs" ActiveX control
comdlg32.ocx, supplied with Visual Basic. Unfortunately, there are different
revisions of this control available which are not binary compatible. In order
to maintain compatibility of the source code with all available versions of
comdlg32.ocx, the source files may have to be edited manually after the
program has been saved from the VB environment on a system containing the
latest release of comdlg32.ocx:

Both vbunzip.frm and vbunzip.vbp should contain a reference line for the
common dialog ocx reading like:
Object = "{F9043C88-F6F2-101A-A3C9-08002B2F49FB}#1.1#0"; "COMDLG32.OCX"
                                                 ^^^
The important section of this line is the revision number "1.1" (marked by
the "^^^" signs). On a system with a more recent version of comdlg32.ocx
installed, this version number is updated to "1.2" (or higher) by VB
automatically. This number has to be changed back to "1.1" manually,
otherwise the example code can no longer be used on systems with old versions
of comdlg32.ocx.

When fetching the VB example code from the UnZip source distribution, one
has to make sure that the files are stored in the correct "DOS/Windows-native"
text format with "CR-LF" line endings. Visual Basic does not accept Unix style
text format (LF line terminators) for Form class modules (*.frm) and the
project file (*.vbp).  You may use unzip's -a option to convert the project
on extraction. If this is a full source distribution, see the Where file for
where to download the compiled unzip DLL for a specific platform which includes
this example project in native format for that platform.

For more information, see the comments within the VB source.
