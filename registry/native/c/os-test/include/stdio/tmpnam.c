/*[OB]*/
#include <stdio.h>
#ifdef tmpnam
#undef tmpnam
#endif
char *(*foo)(char *) = tmpnam;
int main(void) { return 0; }
