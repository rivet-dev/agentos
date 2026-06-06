#include <wchar.h>
#ifdef swscanf
#undef swscanf
#endif
int (*foo)(const wchar_t *restrict, const wchar_t *restrict, ...) = swscanf;
int main(void) { return 0; }
