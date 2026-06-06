#include <threads.h>
#ifdef tss_delete
#undef tss_delete
#endif
void (*foo)(tss_t) = tss_delete;
int main(void) { return 0; }
