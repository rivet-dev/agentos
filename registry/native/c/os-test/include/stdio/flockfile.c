#include <stdio.h>
#ifdef flockfile
#undef flockfile
#endif
void (*foo)(FILE *) = flockfile;
int main(void) { return 0; }
