/*[ADV]*/
#include <stdlib.h>
#ifdef posix_memalign
#undef posix_memalign
#endif
int (*foo)(void **, size_t, size_t) = posix_memalign;
int main(void) { return 0; }
