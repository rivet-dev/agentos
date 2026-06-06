#include <wchar.h>
#ifdef wcscasecmp
#undef wcscasecmp
#endif
int (*foo)(const wchar_t *, const wchar_t *) = wcscasecmp;
int main(void) { return 0; }
