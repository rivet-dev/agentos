#include <stdlib.h>
#ifdef labs
#undef labs
#endif
long (*foo)(long) = labs;
int main(void) { return 0; }
