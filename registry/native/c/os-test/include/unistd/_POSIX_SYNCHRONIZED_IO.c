/*[SIO]*/
#include <unistd.h>
#ifndef _POSIX_SYNCHRONIZED_IO
#error "_POSIX_SYNCHRONIZED_IO is not defined"
#endif
int main(void) { return 0; }
