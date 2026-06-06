#include <stdio.h>
#ifdef popen
#undef popen
#endif
FILE *(*foo)(const char *, const char *) = popen;
int main(void) { return 0; }
