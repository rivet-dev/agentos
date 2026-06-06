#include <stdio.h>
#ifdef clearerr
#undef clearerr
#endif
void (*foo)(FILE *) = clearerr;
int main(void) { return 0; }
