#include <stdlib.h>
#ifdef mkostemp
#undef mkostemp
#endif
int (*foo)(char *, int) = mkostemp;
int main(void) { return 0; }
