#include <dirent.h>
#ifdef rewinddir
#undef rewinddir
#endif
void (*foo)(DIR *) = rewinddir;
int main(void) { return 0; }
