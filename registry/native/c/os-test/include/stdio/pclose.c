#include <stdio.h>
#ifdef pclose
#undef pclose
#endif
int (*foo)(FILE *) = pclose;
int main(void) { return 0; }
