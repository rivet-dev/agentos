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
	/// Summary description for ZipFileEntry.
	/// </summary>
	public class ZipFileEntry
	{

		#region Private Vars

		private string m_FileName;
		private string m_FilePath;
		private bool m_IsFolder;
		private ulong m_FileSize;
		private int m_FileMonth;
		private int m_FileDay;
		private int m_FileYear;
		private int m_FileHour;
		private int m_FileMinute;
		private int m_CompressionFactor;
		private ulong m_CompressedSize;
		private string m_CompressMeth;

		#endregion

		public ZipFileEntry()
		{
		}

		#region Properties

		public string FileName
		{
			get {return m_FileName;}
			set {m_FileName = value;}
		}

		public string FilePath
		{
			get {return m_FilePath;}
			set {m_FilePath = value;}
		}

		public bool IsFolder
		{
			get {return m_IsFolder;}
			set {m_IsFolder = value;}
		}

		public ulong FileSize
		{
			get {return m_FileSize;}
			set {m_FileSize = value;}
		}

		public int FileMonth
		{
			get {return m_FileMonth;}
			set {m_FileMonth = value;}
		}

		public int FileDay
		{
			get {return m_FileDay;}
			set {m_FileDay = value;}
		}

		public int FileYear
		{
			get {return m_FileYear;}
			set {m_FileYear = value;}
		}

		public int FileHour
		{
			get {return m_FileHour;}
			set {m_FileHour = value;}
		}

		public int FileMinute
		{
			get {return m_FileMinute;}
			set {m_FileMinute = value;}
		}

		public int CompressionFactor
		{
			get {return m_CompressionFactor;}
			set {m_CompressionFactor = value;}
		}

		public ulong CompressedSize
		{
			get {return m_CompressedSize;}
			set {m_CompressedSize = value;}
		}

		public string CompressionMethShort
		{
			get {return m_CompressMeth;}
			set {m_CompressMeth = value;}
		}

		#endregion

	}
}
