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
using System.Collections;

namespace CSharpInfoZip_UnZipSample
{
	/// <summary>
	/// Summary description for ZipFileEntries
	/// </summary>
	[Serializable]
	public class ZipFileEntries: CollectionBase, IDisposable
	{

		public ZipFileEntries()
		{
		}

		//the normal collections methods...
		public void Add(ZipFileEntry obj)
		{
			List.Add(obj);
		}

		public void Remove(int index)
		{
			if (index > Count - 1 || index < 0)
			{
				//throw an error here...
			}
			else
			{
				List.RemoveAt(index);
			}
		}

		public ZipFileEntry Item(int Index)
		{
			return (ZipFileEntry) List[Index];
		}


		#region IDisposable Members

		public void Dispose()
		{
			//Any custom dispose logic goes here...
		}

		#endregion



	}
}
