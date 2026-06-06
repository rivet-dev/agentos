#include <stdio.h>
#ifdef tmpfile
#undef tmpfile
#endif
FILE *(*foo)(void) = tmpfile;
int main(void) { return 0; }
