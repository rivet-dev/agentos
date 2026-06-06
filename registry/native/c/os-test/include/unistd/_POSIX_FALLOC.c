/*optional*/
#include <unistd.h>
#ifndef _POSIX_FALLOC
#error "_POSIX_FALLOC is not defined"
#endif
int main(void) { return 0; }
