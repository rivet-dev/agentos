#region README

	//_____________________________________________________________________________
	//
	//Sample C# code, .NET Framework 1.1, contributed to the Info-Zip project by
	//Adrian Maull, April 2005.
	//
	//If you have questions or comments, contact me at adrian.maull@sprintpcs.com.  Though
	//I will try to respond to coments/questions, I do not guarantee such response.
	//
	//THIS CODE AND INFORMATION ARE PROVIDED "AS IS" WITHOUT WARRANTY OF ANY
	//KIND, EITHER EXPRESSED OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
	//IMPLIED WARRANTIES OF MERCHANTABILITY AND/OR FITNESS FOR A
	//PARTICULAR PURPOSE.
	//
	//_____________________________________________________________________________


#endregion

using System;

namespace CSharpInfoZip_UnZipSample
{
	/// <summary>
	/// Summary description for UnZipDLLServiceMessageEventArgs.
	/// </summary>
	public class UnZipDLLServiceMessageEventArgs
	{
		private ulong m_ZipFileSize = 0;
		private ulong m_SizeOfFileEntry = 0;
		private string m_FileEntryName = string.Empty;

		//zipFileSize = Total size of the zip file
		//fileEntryBytes - size of an individual file in the zip
		//fileEntryName - name of an individual file in the zip
		public UnZipDLLServiceMessageEventArgs(ulong zipFileSize, string fileEntryName, ulong fileEntryBytes)
		{
			m_ZipFileSize = zipFileSize;
			m_SizeOfFileEntry = fileEntryBytes;
			m_FileEntryName = fileEntryName;
		}

		public ulong ZipFileSize
		{
			get {return m_ZipFileSize;}
		}

		public ulong SizeOfFileEntry
		{
			get {return m_SizeOfFileEntry;}
		}

		public string FileEntryName
		{
			get {return m_FileEntryName;}
		}
	}
}
